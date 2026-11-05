//! 模型调用的流式 / 超时 / 重试 / 熔断测试：用一个可编排的假 OpenAI 兼容服务代替推理服务。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_llm::{Backend, BackendConfig, ChatMessage, ErrorKind, LlmClient, LlmConfig, LlmError, Priority, Timeouts};
use axum::body::{Body, Bytes};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio_stream::wrappers::ReceiverStream;

// ───────────────────────── 可编排的假服务 ─────────────────────────

type Chunks = Vec<(Duration, Vec<u8>)>;

enum Reply {
    /// 先等 `head_delay` 再发响应头（模拟排队 / 预填充），然后按间隔发送各个数据块；
    /// `hold` 不为空时，发完后保持连接不关闭（模拟卡住）
    Stream { head_delay: Duration, chunks: Chunks, hold: Option<Duration> },
    Status(u16, Vec<(&'static str, &'static str)>, String),
}

struct Mock {
    url: String,
    hits: Arc<Mutex<Vec<(Instant, Value)>>>,
    /// 服务端是否看到过客户端断开
    disconnected: Arc<AtomicBool>,
}

impl Mock {
    fn count(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
    fn body(&self, i: usize) -> Value {
        self.hits.lock().unwrap()[i].1.clone()
    }
    fn gap(&self, a: usize, b: usize) -> Duration {
        let h = self.hits.lock().unwrap();
        h[b].0 - h[a].0
    }
}

async fn start<G>(script: G) -> Mock
where
    G: Fn(usize, &Value) -> Reply + Send + Sync + 'static,
{
    let hits: Arc<Mutex<Vec<(Instant, Value)>>> = Arc::default();
    let disconnected = Arc::new(AtomicBool::new(false));
    let script = Arc::new(script);
    let (h2, d2) = (hits.clone(), disconnected.clone());
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(req): Json<Value>| {
            let (hits, disc, script) = (h2.clone(), d2.clone(), script.clone());
            async move {
                let idx = {
                    let mut h = hits.lock().unwrap();
                    h.push((Instant::now(), req.clone()));
                    h.len() - 1
                };
                match script(idx, &req) {
                    Reply::Status(code, headers, body) => {
                        let mut r = Response::builder().status(code);
                        for (k, v) in headers {
                            r = r.header(k, v);
                        }
                        r.body(Body::from(body)).unwrap()
                    }
                    Reply::Stream { head_delay, chunks, hold } => {
                        tokio::time::sleep(head_delay).await;
                        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
                        tokio::spawn(async move {
                            for (d, bytes) in chunks {
                                tokio::time::sleep(d).await;
                                if tx.send(Ok(Bytes::from(bytes))).await.is_err() {
                                    disc.store(true, Ordering::SeqCst);
                                    return;
                                }
                            }
                            if let Some(h) = hold {
                                let end = Instant::now() + h;
                                while Instant::now() < end {
                                    tokio::time::sleep(Duration::from_millis(30)).await;
                                    if tx.is_closed() {
                                        disc.store(true, Ordering::SeqCst);
                                        return;
                                    }
                                }
                            }
                        });
                        Response::builder().header("content-type", "text/event-stream").body(Body::from_stream(ReceiverStream::new(rx))).unwrap()
                    }
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Mock { url: format!("http://{addr}/v1"), hits, disconnected }
}

// ───────────────────────── 构造 SSE 内容 ─────────────────────────

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn data(v: Value) -> Vec<u8> {
    format!("data: {v}\n\n").into_bytes()
}

fn tok(piece: &str) -> (Duration, Vec<u8>) {
    (ms(0), data(json!({"choices": [{"delta": {"content": piece}}]})))
}

fn finish(reason: &str) -> (Duration, Vec<u8>) {
    (ms(0), data(json!({"choices": [{"delta": {}, "finish_reason": reason}]})))
}

fn done() -> (Duration, Vec<u8>) {
    (ms(0), b"data: [DONE]\n\n".to_vec())
}

/// 正常结束的一次回复
fn ok_reply(pieces: &[&str]) -> Reply {
    let mut chunks: Chunks = pieces.iter().map(|p| tok(p)).collect();
    chunks.push(finish("stop"));
    chunks.push(done());
    Reply::Stream { head_delay: ms(0), chunks, hold: None }
}

// ───────────────────────── 构造客户端 ─────────────────────────

fn cfg(url: &str, retries: u32) -> LlmConfig {
    LlmConfig { base_url: url.into(), model: "m".into(), max_retries: retries, max_tokens: Some(100), ..Default::default() }
}

fn timeouts(first_ms: u64, idle_ms: u64, total_ms: u64) -> Timeouts {
    Timeouts { connect: Duration::from_secs(1), first_token: ms(first_ms), idle: ms(idle_ms), total: ms(total_ms), deadline: None }
}

/// 没绑定后端的客户端（视为前台，网络重试最多 1 次）
fn plain(url: &str, retries: u32, t: Timeouts) -> LlmClient {
    LlmClient::new(cfg(url, retries)).unwrap().with_timeouts(t)
}

/// 绑定到后端的后台客户端（网络重试最多 `retries` 次）
fn background(url: &str, retries: u32, t: Timeouts, backend_cfg: BackendConfig) -> (LlmClient, Arc<Backend>) {
    let backend = Arc::new(Backend::new("test", BackendConfig { base_url: url.into(), ..backend_cfg }));
    let c = LlmClient::new(cfg(url, retries)).unwrap().with_backend(backend.clone(), "general", Priority::Background).with_timeouts(t);
    (c, backend)
}

fn kind(e: &anyhow::Error) -> ErrorKind {
    e.downcast_ref::<LlmError>().unwrap_or_else(|| panic!("不是 LlmError: {e:#}")).kind
}

fn msgs() -> Vec<ChatMessage> {
    vec![ChatMessage::system("系统"), ChatMessage::user("你好")]
}

// ───────────────────────── 流式解析 ─────────────────────────

#[tokio::test]
async fn streams_reasoning_usage_and_finish_reason() {
    let m = start(|_, _| {
        let chunks: Chunks = vec![
            (ms(0), data(json!({"choices": [{"delta": {"role": "assistant", "content": ""}}]}))),
            (ms(0), data(json!({"choices": [{"delta": {"reasoning_content": "想一想"}}]}))),
            tok("你"),
            tok("好"),
            finish("stop"),
            (ms(0), data(json!({"choices": [], "usage": {"prompt_tokens": 7, "completion_tokens": 2}}))),
            done(),
        ];
        Reply::Stream { head_delay: ms(0), chunks, hold: None }
    })
    .await;
    let (c, backend) = background(&m.url, 0, timeouts(2000, 2000, 5000), BackendConfig::default());
    let mut deltas = vec![];
    let out = c.chat_stream(&msgs(), |d| deltas.push(d.to_string())).await.unwrap();
    assert_eq!(out, "你好");
    assert_eq!(deltas.concat(), "你好");
    // 请求统一走流式，并要求带用量
    let body = m.body(0);
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    assert_eq!(body["max_tokens"], 100);
    // 统计里有 token 用量和首 token 时间
    let rec = &backend.stats.recent()[0];
    assert_eq!((rec.outcome, rec.prompt_tokens, rec.completion_tokens), ("ok", Some(7), Some(2)));
    assert!(rec.ttft_ms.is_some());
    assert_eq!(rec.role, "general");
}

#[tokio::test]
async fn inline_think_blocks_are_hidden_while_streaming_and_stripped_from_result() {
    let m = start(|_, _| ok_reply(&["<think>", "嗯", "</think>", "答案"])).await;
    let c = plain(&m.url, 0, timeouts(2000, 2000, 5000));
    let mut deltas = String::new();
    let out = c.chat_stream(&msgs(), |d| deltas.push_str(d)).await.unwrap();
    assert_eq!((out.as_str(), deltas.as_str()), ("答案", "答案"));
    assert_eq!(c.chat(&msgs()).await.unwrap(), "答案");
}

#[tokio::test]
async fn multibyte_character_split_across_packets_is_preserved() {
    let line = data(json!({"choices": [{"delta": {"content": "你好"}}]}));
    let cut = line.iter().position(|b| *b == 0xE4).unwrap() + 1; // 切在"你"的中间
    let m = start(move |_, _| Reply::Stream { head_delay: ms(0), chunks: vec![(ms(0), line[..cut].to_vec()), (ms(20), line[cut..].to_vec()), finish("stop"), done()], hold: None }).await;
    let c = plain(&m.url, 0, timeouts(2000, 2000, 5000));
    assert_eq!(c.chat(&msgs()).await.unwrap(), "你好");
}

#[tokio::test]
async fn extra_body_is_merged_but_cannot_override_core_fields() {
    let m = start(|_, _| ok_reply(&["好"])).await;
    let extra: serde_json::Map<String, Value> = serde_json::from_value(json!({"chat_template_kwargs": {"enable_thinking": false}, "stream": false})).unwrap();
    let (c, _) = background(&m.url, 0, timeouts(2000, 2000, 5000), BackendConfig { extra_body: extra, ..Default::default() });
    c.chat(&msgs()).await.unwrap();
    let body = m.body(0);
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    assert_eq!(body["stream"], true);
}

// ───────────────────────── 超时与重试 ─────────────────────────

#[tokio::test]
async fn first_token_timeout_is_retried_after_a_backoff_not_immediately() {
    let m = start(|i, _| if i == 0 { Reply::Stream { head_delay: Duration::from_secs(3), chunks: vec![], hold: None } } else { ok_reply(&["好"]) }).await;
    let (c, backend) = background(&m.url, 1, timeouts(300, 2000, 5000), BackendConfig::default());
    let started = Instant::now();
    assert_eq!(c.chat(&msgs()).await.unwrap(), "好");
    assert_eq!(m.count(), 2);
    // 第一次 300ms 超时 + 至少 375ms 退避，才会发第二次
    assert!(m.gap(0, 1) >= ms(650), "两次请求间隔 {:?}", m.gap(0, 1));
    assert!(started.elapsed() < Duration::from_secs(3));
    let outcomes: Vec<_> = backend.stats.recent().iter().map(|r| r.outcome).collect();
    assert_eq!(outcomes, ["timeout_first_token", "ok"]);
}

#[tokio::test]
async fn idle_timeout_after_text_was_pushed_is_not_retried_and_server_sees_disconnect() {
    let m = start(|_, _| Reply::Stream { head_delay: ms(0), chunks: vec![tok("半")], hold: Some(Duration::from_secs(5)) }).await;
    let c = plain(&m.url, 2, timeouts(2000, 300, 5000));
    let mut deltas = vec![];
    let err = c.chat_stream(&msgs(), |d| deltas.push(d.to_string())).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::TimeoutIdle);
    assert_eq!(deltas, ["半"]);
    assert_eq!(m.count(), 1, "已经向用户输出了文本，不能再重试");
    tokio::time::sleep(ms(300)).await;
    assert!(m.disconnected.load(Ordering::SeqCst), "超时后必须断开连接，服务端才会取消生成");
}

#[tokio::test]
async fn rate_limit_waits_for_retry_after() {
    let m = start(|i, _| if i == 0 { Reply::Status(429, vec![("retry-after", "1")], "slow down".into()) } else { ok_reply(&["好"]) }).await;
    let (c, _) = background(&m.url, 1, timeouts(2000, 2000, 5000), BackendConfig::default());
    assert_eq!(c.chat(&msgs()).await.unwrap(), "好");
    assert_eq!(m.count(), 2);
    assert!(m.gap(0, 1) >= ms(1000), "没有遵守 Retry-After：{:?}", m.gap(0, 1));
}

#[tokio::test]
async fn context_exceeded_is_not_retried() {
    let body = r#"{"error":{"code":400,"message":"the request exceeds the available context size","type":"exceed_context_size_error"}}"#;
    let m = start(move |_, _| Reply::Status(400, vec![], body.into())).await;
    let (c, _) = background(&m.url, 2, timeouts(2000, 2000, 5000), BackendConfig::default());
    let err = c.chat(&msgs()).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::ContextExceeded);
    assert!(err.to_string().contains("E_LLM_CONTEXT"), "{err}");
    assert_eq!(m.count(), 1);
}

#[tokio::test]
async fn server_errors_use_the_retry_budget_then_fail_with_http_code() {
    let m = start(|_, _| Reply::Status(503, vec![], "busy".into())).await;
    let (c, _) = background(&m.url, 2, timeouts(2000, 2000, 5000), BackendConfig::default());
    let started = Instant::now();
    let err = c.chat(&msgs()).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::Http5xx);
    assert_eq!(m.count(), 3, "1 次首发 + 2 次重试");
    assert!(started.elapsed() >= ms(1000), "重试之间必须有退避：{:?}", started.elapsed());
}

#[tokio::test]
async fn interactive_calls_retry_at_most_once() {
    let m = start(|_, _| Reply::Status(503, vec![], "busy".into())).await;
    let c = plain(&m.url, 2, timeouts(2000, 2000, 5000));
    assert!(c.chat(&msgs()).await.is_err());
    assert_eq!(m.count(), 2);
}

#[tokio::test]
async fn stream_cut_without_end_marker_is_retried() {
    let m = start(|i, _| if i == 0 { Reply::Stream { head_delay: ms(0), chunks: vec![tok("半截")], hold: None } } else { ok_reply(&["完整"]) }).await;
    let c = plain(&m.url, 1, timeouts(2000, 2000, 5000));
    // chat() 没有向外推送增量文本，所以可以安全重试
    assert_eq!(c.chat(&msgs()).await.unwrap(), "完整");
    assert_eq!(m.count(), 2);
}

#[tokio::test]
async fn call_deadline_stops_retries_and_caps_the_first_token_wait() {
    let m = start(|_, _| Reply::Stream { head_delay: Duration::from_secs(10), chunks: vec![], hold: None }).await;
    let c = plain(&m.url, 2, Timeouts { deadline: Some(Duration::from_secs(1)), ..timeouts(10_000, 2000, 20_000) });
    let started = Instant::now();
    let err = c.chat(&msgs()).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::TimeoutFirstToken);
    assert_eq!(m.count(), 1, "期限内剩余时间不够退避，不应再重试");
    assert!(started.elapsed() < Duration::from_millis(2500), "{:?}", started.elapsed());
}

#[tokio::test]
async fn total_timeout_limits_a_slow_but_alive_stream() {
    let chunks: Chunks = (0..20).map(|_| (ms(100), data(json!({"choices": [{"delta": {"content": "字"}}]})))).collect();
    let m = start(move |_, _| Reply::Stream { head_delay: ms(0), chunks: chunks.clone(), hold: None }).await;
    let (c, _) = background(&m.url, 0, timeouts(2000, 1000, 600), BackendConfig::default());
    let err = c.chat(&msgs()).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::TimeoutTotal);
}

// ───────────────────────── 取消 ─────────────────────────

#[tokio::test]
async fn dropping_a_call_disconnects_and_is_recorded_as_cancelled() {
    let m = start(|_, _| Reply::Stream { head_delay: ms(0), chunks: vec![tok("开头")], hold: Some(Duration::from_secs(5)) }).await;
    let (c, backend) = background(&m.url, 0, timeouts(2000, 4000, 8000), BackendConfig::default());
    let r = tokio::time::timeout(ms(300), c.chat_stream(&msgs(), |_| {})).await;
    assert!(r.is_err(), "调用应该还在进行中");
    tokio::time::sleep(ms(300)).await;
    assert!(m.disconnected.load(Ordering::SeqCst), "调用方放弃后服务端必须看到断开");
    assert_eq!(backend.stats.recent().last().unwrap().outcome, "cancelled");
}

// ───────────────────────── 熔断 ─────────────────────────

#[tokio::test]
async fn breaker_opens_after_three_connection_failures_and_fails_fast() {
    // 拿一个没人监听的端口
    let port = {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let url = format!("http://127.0.0.1:{port}/v1");
    let (c, backend) = background(&url, 0, timeouts(1000, 1000, 2000), BackendConfig::default());
    for _ in 0..3 {
        assert_eq!(kind(&c.chat(&msgs()).await.unwrap_err()), ErrorKind::Unreachable);
    }
    let started = Instant::now();
    let err = c.chat(&msgs()).await.unwrap_err();
    assert_eq!(kind(&err), ErrorKind::BreakerOpen);
    assert!(err.to_string().contains("E_LLM_BREAKER"), "{err}");
    assert!(started.elapsed() < ms(100), "熔断时不应该再发请求");
    let s = backend.stats.summary();
    assert_eq!((s.outcomes["unreachable"], s.outcomes["breaker_open"]), (3, 1));
}

// ───────────────────────── 结构化输出 ─────────────────────────

fn json_cfg_reply(content: &str, reason: &str) -> Reply {
    Reply::Stream { head_delay: ms(0), chunks: vec![tok(content), finish(reason), done()], hold: None }
}

async fn ask(c: &LlmClient) -> Result<an_llm::Structured<Value>, an_llm::StructuredError> {
    c.chat_json::<Value, _>(&msgs(), "t", &json!({"type": "object"}), |_| Ok(())).await
}

#[tokio::test]
async fn truncated_json_doubles_max_tokens_once_without_feeding_back_the_partial_output() {
    let m = start(|i, _| if i == 0 { json_cfg_reply("{\"a\":", "length") } else { json_cfg_reply("{\"a\":1}", "stop") }).await;
    let c = plain(&m.url, 2, timeouts(2000, 2000, 5000));
    let r = ask(&c).await.unwrap();
    assert_eq!(r.value["a"], 1);
    assert_eq!((m.count(), m.body(0)["max_tokens"].clone(), m.body(1)["max_tokens"].clone()), (2, json!(100), json!(200)));
    assert_eq!(m.body(1)["messages"], m.body(0)["messages"], "半截输出不能拼回提示词");
}

#[tokio::test]
async fn still_truncated_after_escalation_reports_truncated_code() {
    let m = start(|_, _| json_cfg_reply("{\"a\":", "length")).await;
    let c = plain(&m.url, 2, timeouts(2000, 2000, 5000));
    let e = ask(&c).await.unwrap_err();
    assert_eq!(e.code, "E_LLM_TRUNCATED");
    assert!(e.message.contains("max_tokens"), "{}", e.message);
    assert_eq!(m.count(), 2);
}

#[tokio::test]
async fn network_failures_do_not_multiply_into_format_retries() {
    let m = start(|_, _| Reply::Status(503, vec![], "busy".into())).await;
    let c = plain(&m.url, 2, timeouts(2000, 2000, 5000));
    let e = ask(&c).await.unwrap_err();
    assert_eq!(e.code, "E_LLM_HTTP");
    assert_eq!(m.count(), 2, "前台：首发 + 最多 1 次重试，而不是 3 次");
}

#[tokio::test]
async fn format_errors_still_feed_back_and_retry() {
    let m = start(|i, _| if i == 0 { json_cfg_reply("不是 json", "stop") } else { json_cfg_reply("{\"a\":2}", "stop") }).await;
    let c = plain(&m.url, 2, timeouts(2000, 2000, 5000));
    let r = ask(&c).await.unwrap();
    assert_eq!((r.value["a"].clone(), r.attempts, r.raw_outputs.len()), (json!(2), 2, 2));
    let last = m.body(1)["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
    assert!(last.contains("上面的输出不合格"), "{last}");
}

#[tokio::test]
async fn format_failure_keeps_the_format_code() {
    let m = start(|_, _| json_cfg_reply("还是不是 json", "stop")).await;
    let c = plain(&m.url, 1, timeouts(2000, 2000, 5000));
    let e = ask(&c).await.unwrap_err();
    assert_eq!((e.code, e.attempts, m.count()), ("E_LLM_FORMAT", 2, 2));
}
