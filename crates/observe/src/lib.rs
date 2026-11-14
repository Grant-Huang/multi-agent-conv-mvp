//! 日志与诊断（埋点）。
//!
//! 三个输出：
//! 1. **控制台**：给人看，简洁；
//! 2. **JSON 日志文件**（`logs/<程序名>.jsonl.<日期>`，按天滚动）：给程序看，一行一个事件，
//!    字段稳定，方便用脚本或 AI 自动排查；
//! 3. **最近问题缓冲**：内存中保留最近的 WARN / ERROR 事件，通过诊断接口（`/api/diagnostics`、
//!    `/node/diagnostics`）直接查询，不用翻日志文件。
//!
//! 埋点约定（所有代码都按这个写，排查脚本才能可靠地过滤）：
//!
//! | 字段 | 含义 | 例子 |
//! |---|---|---|
//! | `event` | 事件名，`模块.动作` | `task.finished`、`acp.permission` |
//! | `code` | 错误码，只在出问题时有 | `E_ACP_HANDSHAKE`、`W_POOL_WAIT` |
//! | `conv_id` / `task_id` | 关联的会话 / 任务 | |
//! | `agent_id` / `node_id` | 关联的 agent / 节点 | `hermes@mac-mini` |
//! | `elapsed_ms` | 耗时 | |

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use ts_rs::TS;

/// 一条"问题"记录（WARN / ERROR 事件）
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Problem {
    pub time: String,
    /// "WARN" 或 "ERROR"
    pub level: String,
    /// 产生日志的模块
    pub target: String,
    pub event: Option<String>,
    pub code: Option<String>,
    pub message: String,
    /// 其余字段（task_id、agent_id、error 等）
    pub fields: BTreeMap<String, String>,
}

/// 最近问题的环形缓冲
pub struct Diagnostics {
    buf: Mutex<VecDeque<Problem>>,
    capacity: usize,
}

impl Diagnostics {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            buf: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        })
    }

    fn push(&self, p: Problem) {
        let mut b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        if b.len() == self.capacity {
            b.pop_front();
        }
        b.push_back(p);
    }

    /// 最近的问题，新的在前
    pub fn recent(&self, limit: usize) -> Vec<Problem> {
        let b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        b.iter().rev().take(limit).cloned().collect()
    }

    /// 按错误码统计次数（快速看出哪类问题最多）
    pub fn counts_by_code(&self) -> BTreeMap<String, usize> {
        let b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        let mut m = BTreeMap::new();
        for p in b.iter() {
            *m.entry(p.code.clone().unwrap_or_else(|| "（无错误码）".into()))
                .or_insert(0) += 1;
        }
        m
    }
}

/// 把事件字段收集成字符串表
#[derive(Default)]
struct FieldCollector {
    message: String,
    fields: BTreeMap<String, String>,
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let v = format!("{value:?}");
        if field.name() == "message" {
            self.message = v;
        } else {
            // Debug 格式的字符串带引号，去掉
            self.fields
                .insert(field.name().to_string(), v.trim_matches('"').to_string());
        }
    }
}

/// 把 WARN / ERROR 事件写入诊断缓冲的 tracing 层
struct DiagnosticsLayer(Arc<Diagnostics>);

impl<S: Subscriber> Layer<S> for DiagnosticsLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        if level > Level::WARN {
            return;
        }
        let mut c = FieldCollector::default();
        event.record(&mut c);
        let event_name = c.fields.remove("event");
        let code = c.fields.remove("code");
        self.0.push(Problem {
            time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: level.to_string(),
            target: event.metadata().target().to_string(),
            event: event_name,
            code,
            message: c.message,
            fields: c.fields,
        });
    }
}

/// 日志配置
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// JSON 日志目录；None 表示不写文件
    pub dir: Option<PathBuf>,
    /// 默认日志级别（可被 RUST_LOG 覆盖）
    pub level: String,
}

/// 日志系统的句柄：必须在程序运行期间一直持有，否则文件日志可能丢最后几条
pub struct LogGuard {
    _file: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// 第一次初始化时创建的诊断缓冲（日志系统全局只能初始化一次）
static GLOBAL: std::sync::OnceLock<Arc<Diagnostics>> = std::sync::OnceLock::new();

/// 初始化日志。`app` 用作日志文件名，例如 `agentnexus` → `logs/agentnexus.jsonl.2026-11-05`。
/// 重复调用（例如测试中）时不会重复初始化，返回第一次创建的诊断缓冲。
pub fn init(app: &str, cfg: &LogConfig) -> (Arc<Diagnostics>, LogGuard) {
    if let Some(d) = GLOBAL.get() {
        return (d.clone(), LogGuard { _file: None });
    }
    let diagnostics = GLOBAL.get_or_init(|| Diagnostics::new(500)).clone();
    let filter = || {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(format!("{},tower_http=warn,mdns_sd=warn", cfg.level))
        })
    };

    let console = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(filter());
    // Builder returns initialization errors instead of panicking (e.g. logs is a file).
    let appender = cfg.dir.as_ref().map(|dir| {
        tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix(format!("{app}.jsonl"))
            .build(dir)
    });
    let (file_layer, guard, file_error) = match appender {
        Some(Ok(appender)) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            // flatten_event：字段直接放在顶层，方便 jq / 脚本过滤，例如 jq 'select(.code != null)'
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(false)
                .with_writer(writer)
                .with_filter(filter());
            (Some(layer), Some(guard), None)
        }
        Some(Err(error)) => (None, None, Some(error)),
        None => (None, None, None),
    };

    // 其他代码（例如测试框架）可能已经设置过全局日志，忽略"已初始化"的错误
    let _ = tracing_subscriber::registry()
        .with(console)
        .with(file_layer)
        .with(DiagnosticsLayer(diagnostics.clone()))
        .try_init();
    if let Some(error) = file_error {
        tracing::warn!(event = "log.file_unavailable", task_id = "", error = %error,
            "文件日志不可用，已降级为控制台和内存诊断");
    }
    (diagnostics, LogGuard { _file: guard })
}

/// 只启用诊断缓冲（测试用，不输出到控制台和文件）
pub fn diagnostics_only() -> Arc<Diagnostics> {
    Diagnostics::new(500)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_warn_and_error_with_fields() {
        let d = Diagnostics::new(2);
        let sub = tracing_subscriber::registry().with(DiagnosticsLayer(d.clone()));
        tracing::subscriber::with_default(sub, || {
            tracing::info!(event = "x.ok", "不会被记录");
            tracing::warn!(
                event = "pool.wait",
                code = "W_POOL_WAIT",
                task_id = "t1",
                "等待空闲 agent"
            );
            tracing::error!(
                event = "acp.handshake_failed",
                code = "E_ACP_HANDSHAKE",
                agent_id = "mock",
                "握手失败"
            );
            tracing::error!(event = "e3", code = "E_X", "第三条");
        });
        let r = d.recent(10);
        assert_eq!(r.len(), 2, "容量为 2，最旧的被丢弃");
        assert_eq!(r[0].code.as_deref(), Some("E_X"));
        assert_eq!(r[1].event.as_deref(), Some("acp.handshake_failed"));
        assert_eq!(
            r[1].fields.get("agent_id").map(String::as_str),
            Some("mock")
        );
        assert_eq!(r[1].message, "握手失败");
        assert_eq!(d.counts_by_code().get("E_ACP_HANDSHAKE"), Some(&1));
    }
}
