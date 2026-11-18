//! 用例 RT-AGDIAG-05、07、08、10、11、12 与 RT-DLP-01～06（定义见 docs/testing/CASES.md
//! 「助手排查：诊断包与请其他助手帮忙」）。
//!
//! - 模拟的 DLP 服务：`tests/fixtures/dlp_server.py`（MCP stdio，按 docs/dlp/CONTRACT.md 返回），
//!   行为由环境变量 DLP_MODE / DLP_LOG 控制；
//! - 帮手：`an-mock-acp`，用环境变量 AN_MOCK_ACP_RECORD 记录它收到的提示词、工作目录和进程号，
//!   AN_MOCK_ACP_REPLY 在回复末尾追加文字；
//! - 出问题的助手：沙箱里的 sh 脚本，错误输出里带家目录、用户名、主机名和客户名 ACME。
//!
//! HOME / USER / DLP_* 在整条用例期间指向沙箱，用例之间用同一把锁串行；本文件是独立测试二进制。

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

const MOCK: &str = env!("CARGO_BIN_EXE_an-mock-acp");
/// 给帮手的说明文字里一定会出现的一句（用来确认日志里没有待发内容）
const INSTRUCTION_MARK: &str = "请用简体中文";

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ───────────── 日志捕获：确认日志里没有待发内容的原文 ─────────────

#[derive(Clone)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for LogBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 全局日志（整个测试二进制共用一份）
fn logs() -> &'static LogBuf {
    static LOGS: OnceLock<LogBuf> = OnceLock::new();
    LOGS.get_or_init(|| {
        let buf = LogBuf(Arc::default());
        let writer = buf.clone();
        let _ = tracing_subscriber::fmt()
            .with_env_filter("info")
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .try_init();
        buf
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&logs().0.lock().unwrap()).into_owned()
}

// ───────────── 沙箱 ─────────────

struct Sandbox {
    root: PathBuf,
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

/// 本机主机名（Linux 读 /proc，其他系统用 uname -n）
fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            std::process::Command::new("uname")
                .arg("-n")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 在原 PATH 里找 python3（模拟 DLP 服务用）
fn python3() -> String {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join("python3"))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
        .expect("测试需要 python3")
}

impl Sandbox {
    /// `dlp_mode`：模拟 DLP 的模式（anonymize / review / bad_output / bad_version / slow）
    fn new(name: &str, dlp_mode: &str) -> Self {
        logs();
        let root = std::env::temp_dir().join(an_core::new_id(&format!("agdiag-send-{name}")));
        for d in ["bin", "home/proj", "files", "code", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let mut old = vec![];
        for (k, v) in [
            ("HOME", root.join("home").into_os_string()),
            ("USER", "zhangsan".into()),
            ("DLP_MODE", dlp_mode.into()),
            ("DLP_LOG", root.join("dlp-log.jsonl").into_os_string()),
        ] {
            old.push((k, std::env::var_os(k)));
            std::env::set_var(k, v);
        }
        Self { root, old }
    }

    /// 出问题的助手：把 `extra` 和家目录、用户名、主机名、客户名一起写到错误输出后退出
    fn broken(&self, extra: &str) -> PathBuf {
        let p = self.root.join("bin").join("broken");
        let body = format!(
            "echo \"启动失败：$HOME/proj/config.json 属于 zhangsan@{host}，客户 ACME 的配置不对。{extra}\" >&2\nexit 1",
            host = hostname()
        );
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    fn record_path(&self) -> PathBuf {
        self.root.join("record.json")
    }

    /// 帮手收到的内容（没有被启动时为 None）
    fn record(&self) -> Option<Value> {
        std::fs::read_to_string(self.record_path())
            .ok()
            .map(|s| serde_json::from_str(&s).unwrap())
    }

    /// 模拟 DLP 收到的调用（工具名、task_id 等，不含文本）
    fn dlp_log(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.join("dlp-log.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// 配置：出问题的助手 broken、帮手 helper（已连通）、未连通的 helper2；`with_dlp` 时加上 [egress]（mode = audit，指向模拟 DLP 服务）
    async fn serve(
        &self,
        broken_extra: &str,
        with_dlp: bool,
        dlp_timeout: u64,
    ) -> (String, Arc<Runtime>) {
        let broken = self.broken(broken_extra);
        let dlp = if with_dlp {
            format!(
                "[egress]\nmode = \"audit\"\ndlp_command = \"{py}\"\ndlp_args = [\"{fixture}\"]\ntimeout_ms = {ms}\n",
                py = python3(),
                ms = dlp_timeout * 1000,
                fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/dlp_server.py")
                    .display(),
            )
        } else {
            String::new()
        };
        let text = format!(
            r#"
[server]
db_path = "{data}/agentnexus.db"
[llm]
base_url = "http://127.0.0.1:9/v1"
model = "m"
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
workdir = "{code}"
[acp]
auto_detect = false
[[acp.agents]]
id = "broken"
name = "坏掉的助手"
command = "{broken}"
role = "coding"
env = {{ API_TOKEN = "value-123" }}
[[acp.agents]]
id = "helper"
name = "帮手"
command = "{mock}"
role = "coding"
env = {{ AN_MOCK_ACP_RECORD = "{record}", AN_MOCK_ACP_REPLY = "建议检查 [CUSTOMER_001] 的配置文件" }}
[[acp.agents]]
id = "helper2"
name = "没连通的帮手"
command = "{mock}"
role = "coding"
{dlp}
"#,
            data = self.root.join("data").display(),
            files = self.root.join("files").display(),
            code = self.root.join("code").display(),
            broken = broken.display(),
            mock = MOCK,
            record = self.record_path().display(),
        );
        let (config, _) = AppConfig::parse(&text).unwrap();
        let rt =
            // 开启 [egress] 时要用异步构造（启动外发检查守卫）
            Arc::new(
                Runtime::with_mcp_diagnostics(
                    config,
                    Store::open(Path::new(":memory:")).unwrap(),
                    an_observe::Diagnostics::new(16),
                )
                .await
                .unwrap(),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let for_serve = rt.clone();
        tokio::spawn(async move {
            axum::serve(listener, an_server::api(for_serve))
                .await
                .unwrap()
        });
        let base = format!("http://{addr}");
        // 帮手先握手一次，成为"已连通"
        let (status, out) = post(&base, "/api/agents/probe", json!({"id": "helper"})).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(out["kind"], "ok", "{out}");
        (base, rt)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (k, v) in self.old.drain(..) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn post(base: &str, path: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}{path}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn preview(base: &str, helper: &str) -> (u16, Value) {
    post(
        base,
        "/api/agents/diagnose/preview",
        json!({"id": "broken", "helper": helper}),
    )
    .await
}

async fn send(base: &str, p: &Value) -> (u16, Value) {
    post(
        base,
        "/api/agents/diagnose/send",
        json!({"consent_id": p["consent_id"], "consent_token": p["consent_token"]}),
    )
    .await
}

/// 进程是否已经结束（Linux 看 /proc）
fn wait_dead(pid: u64, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

// ───────────── 用例 ─────────────

/// RT-AGDIAG-05（没有配置 DLP）：诊断包只经本机处理，注明"未经外发脱敏检查"
#[tokio::test]
async fn rt_agdiag_05_package_without_dlp() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("05a", "anonymize");
    let (base, _rt) = sb.serve("", false, 3).await;
    let (status, report) = post(&base, "/api/agents/diagnose", json!({"id": "broken"})).await;
    assert_eq!(status, 200, "{report}");
    let pkg = &report["package"];
    let text = pkg["text"]
        .as_str()
        .unwrap_or_else(|| panic!("应有诊断包：{report}"));
    let home = sb.root.join("home").display().to_string();
    assert!(
        text.contains("~/proj/config.json"),
        "家目录应换成 ~：{text}"
    );
    assert!(!text.contains(&home), "不应出现家目录完整路径：{text}");
    assert!(!text.contains("zhangsan"), "不应出现用户名：{text}");
    let host = hostname();
    assert!(
        host.is_empty() || !text.contains(&host),
        "不应出现主机名 {host}：{text}"
    );
    assert!(!text.contains("value-123"), "不应出现环境变量的值：{text}");
    assert!(text.contains("API_TOKEN"), "环境变量应保留名字：{text}");
    assert_eq!(pkg["dlp_checked"], false, "{pkg}");
    assert!(
        pkg["note"]
            .as_str()
            .is_some_and(|n| n.contains("未经外发脱敏检查")),
        "{pkg}"
    );
    assert!(sb.dlp_log().is_empty());
}

/// RT-AGDIAG-05（配置了 DLP）：诊断包是 DLP 脱敏后的文本，渠道 clipboard，用完即撤销
#[tokio::test]
async fn rt_agdiag_05_package_with_dlp() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("05b", "anonymize");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, report) = post(&base, "/api/agents/diagnose", json!({"id": "broken"})).await;
    assert_eq!(status, 200, "{report}");
    let pkg = &report["package"];
    let text = pkg["text"]
        .as_str()
        .unwrap_or_else(|| panic!("应有诊断包：{report}"));
    assert!(
        text.contains("[CUSTOMER_001]") && !text.contains("ACME"),
        "{text}"
    );
    assert!(
        !text.contains("zhangsan") && text.contains("~/proj"),
        "{text}"
    );
    assert_eq!(pkg["dlp_checked"], true, "{pkg}");
    let log = sb.dlp_log();
    let eval = log
        .iter()
        .find(|c| c["tool"] == "egress_evaluate")
        .unwrap_or_else(|| panic!("应调用 egress_evaluate：{log:?}"));
    assert_eq!(eval["channel"], "clipboard");
    assert_eq!(eval["destination_id"], "user:clipboard");
    assert_eq!(eval["purpose"], "diagnose");
    assert!(
        log.iter()
            .any(|c| c["tool"] == "egress_revoke" && c["task_id"] == eval["task_id"]),
        "用完应撤销：{log:?}"
    );
}

/// RT-AGDIAG-07：预览即所发；不带凭证、凭证错误、用过、过期都拒绝，且不启动帮手
#[tokio::test]
async fn rt_agdiag_07_preview_equals_sent_and_consent_rules() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("07", "anonymize");
    let (base, rt) = sb.serve("", true, 3).await;

    let (status, p) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p}");
    let content = p["content"].as_str().unwrap().to_string();
    assert!(content.contains(INSTRUCTION_MARK), "{content}");
    assert!(
        p["destination"]
            .as_str()
            .is_some_and(|d| d.contains("帮手") && d.contains("模型服务")),
        "目的地说明应写明帮手及其模型服务：{p}"
    );
    assert!(
        p["expires_in_secs"]
            .as_u64()
            .is_some_and(|s| s > 0 && s <= 300),
        "{p}"
    );

    // 不带凭证、凭证错误：拒绝，帮手没有被启动
    let (status, _) = post(
        &base,
        "/api/agents/diagnose/send",
        json!({"consent_id": p["consent_id"]}),
    )
    .await;
    assert!(status >= 400, "不带凭证应被拒绝");
    let (status, out) = post(
        &base,
        "/api/agents/diagnose/send",
        json!({"consent_id": p["consent_id"], "consent_token": "wrong"}),
    )
    .await;
    assert_eq!(status, 403, "{out}");
    assert!(sb.record().is_none(), "凭证无效时不应启动帮手");

    // 正确的凭证：帮手收到的内容与预览一字不差
    let (status, out) = send(&base, &p).await;
    assert_eq!(status, 200, "{out}");
    let rec = sb.record().expect("帮手应被启动");
    assert_eq!(
        rec["prompt"].as_str().unwrap(),
        content,
        "发出的内容应与预览一致"
    );

    // 凭证用过：拒绝，帮手不再被启动
    std::fs::remove_file(sb.record_path()).unwrap();
    let (status, out) = send(&base, &p).await;
    assert!(
        status == 404 || status == 410,
        "凭证重复使用应被拒绝：{status} {out}"
    );
    assert!(sb.record().is_none());

    // 凭证过期：拒绝，帮手不被启动
    let (status, p2) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p2}");
    rt.expire_diagnose_consents();
    let (status, out) = send(&base, &p2).await;
    assert_eq!(status, 410, "{out}");
    assert!(sb.record().is_none());
}

/// RT-DLP-02：占位符只出现在发出去的内容里；帮手回复里的占位符还原后（local_ui）才显示给用户
#[tokio::test]
async fn rt_dlp_02_tokens_sent_and_restored_for_display() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("dlp02", "anonymize");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, p) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p}");
    let content = p["content"].as_str().unwrap();
    assert!(
        content.contains("[CUSTOMER_001]") && !content.contains("ACME"),
        "预览里只应有占位符：{content}"
    );
    let (status, out) = send(&base, &p).await;
    assert_eq!(status, 200, "{out}");
    let sent = sb.record().unwrap()["prompt"].as_str().unwrap().to_string();
    assert!(
        sent.contains("[CUSTOMER_001]") && !sent.contains("ACME"),
        "发出去的内容里只应有占位符：{sent}"
    );
    let reply = out["reply"].as_str().unwrap();
    assert!(
        reply.contains("ACME 的配置文件"),
        "回复应还原后显示：{reply}"
    );
    assert!(!reply.contains("[CUSTOMER_001]"), "{reply}");
    assert!(
        sb.dlp_log()
            .iter()
            .any(|c| c["tool"] == "egress_restore" && c["context"] == "local_ui"),
        "还原只用于本机显示"
    );
}

/// RT-DLP-06：发送结束后用同一个 task_id 撤销；日志里没有待发内容的原文
#[tokio::test]
async fn rt_dlp_06_revoke_after_send_and_no_content_in_logs() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("dlp06", "anonymize");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, p) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p}");
    let (status, out) = send(&base, &p).await;
    assert_eq!(status, 200, "{out}");
    let log = sb.dlp_log();
    let eval = log
        .iter()
        .find(|c| c["tool"] == "egress_evaluate" && c["channel"] == "agent")
        .unwrap_or_else(|| panic!("{log:?}"));
    assert_eq!(eval["destination_id"], "agent:helper");
    assert_eq!(eval["purpose"], "diagnose");
    assert!(
        log.iter()
            .any(|c| c["tool"] == "egress_revoke" && c["task_id"] == eval["task_id"]),
        "应撤销：{log:?}"
    );
    let logs = log_text();
    assert!(logs.contains("agent.diagnose_sent"), "应记录外发事件");
    // 事件名以 DESIGN 第 10 节为准（合并 master 的外发守卫后统一为 egress.evaluate）
    assert!(logs.contains("egress.evaluate"), "应记录评估事件");
    assert!(!logs.contains(INSTRUCTION_MARK), "日志里不应有待发内容");
    assert!(!logs.contains("ACME"), "日志里不应有原文");
}

/// RT-AGDIAG-08：帮手的权限请求一律拒绝，工作目录是临时空目录，结束后删除
#[tokio::test]
async fn rt_agdiag_08_helper_deny_all_in_temp_dir() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("08", "anonymize");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, p) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p}");
    let (status, out) = send(&base, &p).await;
    assert_eq!(status, 200, "{out}");
    assert!(
        out["reply"]
            .as_str()
            .unwrap()
            .contains("没有获得写文件权限"),
        "权限应被拒绝：{out}"
    );
    let rec = sb.record().unwrap();
    let cwd = PathBuf::from(rec["cwd"].as_str().unwrap());
    assert!(
        !cwd.starts_with(sb.root.join("code")),
        "不应在代码目录运行：{cwd:?}"
    );
    assert!(!cwd.exists(), "临时目录应已删除：{cwd:?}");
}

/// RT-AGDIAG-10：帮手超时 → 大白话说明，进程已结束，映射也已撤销
#[tokio::test]
async fn rt_agdiag_10_helper_timeout() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("10", "anonymize");
    let (base, rt) = sb.serve("要等很久，慢", true, 3).await;
    rt.set_diagnose_helper_timeout(Duration::from_secs(2));
    let (status, p) = preview(&base, "helper").await;
    assert_eq!(status, 200, "{p}");
    let (status, out) = send(&base, &p).await;
    assert_eq!(status, 504, "{out}");
    assert!(
        out["error"].as_str().is_some_and(|e| e.contains("秒")),
        "应说明超时：{out}"
    );
    let pid = sb.record().unwrap()["pid"].as_u64().unwrap();
    assert!(wait_dead(pid, 5), "帮手进程应已结束");
    let log = sb.dlp_log();
    let eval = log
        .iter()
        .find(|c| c["tool"] == "egress_evaluate" && c["channel"] == "agent")
        .unwrap();
    assert!(log
        .iter()
        .any(|c| c["tool"] == "egress_revoke" && c["task_id"] == eval["task_id"]));
}

/// RT-AGDIAG-11：同时只能有一次发送
#[tokio::test]
async fn rt_agdiag_11_one_send_at_a_time() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("11", "anonymize");
    let (base, _rt) = sb.serve("请稍慢一点", true, 3).await;
    let (_, p1) = preview(&base, "helper").await;
    let (_, p2) = preview(&base, "helper").await;
    let b = base.clone();
    let first = tokio::spawn(async move { send(&b, &p1).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    let (status, out) = send(&base, &p2).await;
    assert_eq!(status, 409, "{out}");
    assert!(
        out["error"].as_str().is_some_and(|e| e.contains("稍后")),
        "{out}"
    );
    let (status, out) = first.await.unwrap();
    assert_eq!(status, 200, "{out}");
}

/// RT-AGDIAG-12：出问题的那个、未连通的、未知的助手都不能当帮手
#[tokio::test]
async fn rt_agdiag_12_helper_must_be_another_connected_agent() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("12", "anonymize");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, out) = preview(&base, "broken").await;
    assert_eq!(status, 409, "{out}");
    let (status, out) = preview(&base, "helper2").await;
    assert_eq!(status, 409, "{out}");
    assert!(
        out["error"].as_str().is_some_and(|e| e.contains("连通")),
        "{out}"
    );
    let (status, out) = preview(&base, "nope").await;
    assert_eq!(status, 404, "{out}");
    assert!(
        sb.dlp_log().iter().all(|c| c["channel"] != "agent"),
        "不应评估外发内容"
    );
}

/// RT-DLP-01：DLP 的工具不出现在能力目录里
#[tokio::test]
async fn rt_dlp_01_tools_not_visible_to_models() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("dlp01", "anonymize");
    let (base, rt) = sb.serve("", true, 3).await;
    let caps = reqwest::get(format!("{base}/api/capabilities"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !caps.contains("egress"),
        "能力目录里不应有 DLP 工具：{caps}"
    );
    assert!(rt
        .capabilities
        .iter()
        .all(|c| !c.tool_id.contains("egress") && !c.capability_id.contains("egress")));
}

/// RT-DLP-03：有密钥 → 拦下，不生成预览
#[tokio::test]
async fn rt_dlp_03_secret_blocked() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("dlp03", "anonymize");
    let (base, _rt) = sb.serve("token=sk-live-abc123", true, 3).await;
    let (status, out) = preview(&base, "helper").await;
    assert_eq!(status, 409, "{out}");
    let e = out["error"].as_str().unwrap_or_default();
    assert!(e.contains("密钥") && e.contains("更换"), "{out}");
    assert!(out.get("consent_id").is_none());
    assert!(sb.record().is_none());
}

/// RT-DLP-04：需要人工判断 → 不生成预览
#[tokio::test]
async fn rt_dlp_04_review_not_sent() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("dlp04", "review");
    let (base, _rt) = sb.serve("", true, 3).await;
    let (status, out) = preview(&base, "helper").await;
    assert_eq!(status, 409, "{out}");
    assert!(
        out["error"].as_str().is_some_and(|e| e.contains("人工")),
        "{out}"
    );
    assert!(sb.record().is_none());
}

/// RT-DLP-05：没有配置、超时、结构不合规、版本不对 → 都不发送；第 1～3 层照常可用
#[tokio::test]
async fn rt_dlp_05_fail_closed() {
    let _g = ENV_LOCK.lock().await;
    // 没有配置 [egress]
    {
        let sb = Sandbox::new("dlp05a", "anonymize");
        let (base, _rt) = sb.serve("", false, 3).await;
        let (status, out) = preview(&base, "helper").await;
        assert_eq!(status, 409, "{out}");
        assert!(
            out["error"]
                .as_str()
                .is_some_and(|e| e.contains("没有配置外发脱敏")),
            "{out}"
        );
        let (status, report) = post(&base, "/api/agents/diagnose", json!({"id": "broken"})).await;
        assert_eq!(status, 200, "{report}");
        assert!(sb.record().is_none());
    }
    for mode in ["slow", "bad_output", "bad_version"] {
        let sb = Sandbox::new(&format!("dlp05-{mode}"), mode);
        let (base, _rt) = sb.serve("", true, 1).await;
        let (status, out) = preview(&base, "helper").await;
        assert_eq!(status, 409, "{mode}：{out}");
        assert!(
            out["error"]
                .as_str()
                .is_some_and(|e| e.contains("外发脱敏")),
            "{mode}：{out}"
        );
        assert!(sb.record().is_none(), "{mode}：不应启动帮手");
        let (status, _) = post(&base, "/api/agents/diagnose", json!({"id": "broken"})).await;
        assert_eq!(status, 200, "{mode}：排查本身应照常可用");
    }
    let _ = std::io::stdout().flush();
}
