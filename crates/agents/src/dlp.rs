//! 外发脱敏（DLP）客户端：按 `docs/dlp/CONTRACT.md` 调用用户的 `tools/dlp-egress`（stdio MCP）。
//!
//! 设计要点（见 docs/DECISIONS.md 第 15～17 条）：
//! - **只由 Rust 代码调用**：工具表只存在于这里，不进任何模型可见的工具清单或能力目录。
//!   `egress_restore` 能把占位符还原成原文，交给模型就可能被注入的内容借来取回敏感信息；
//! - **任何失败都不发**：没配置、启动失败、超时、返回结构不对、版本不对，一律按"不能发"处理；
//! - 复用 MCP 网关（[`crate::mcp::connect`]）：工具的输入、输出都按服务端声明的 schema 校验；
//! - 连接按需建立；某次调用失败后丢掉连接，下次调用重新连接（不会复用可能已损坏的连接）。
//!
//! 超时与清理：每次调用最多 `timeout_secs` 秒（网关内实现）；DLP 子进程随连接释放而结束。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::capability::Effect;
use crate::mcp::{connect, McpServerConfig, McpTransport, ToolBinding};
use crate::tools::ToolRegistry;

/// AgentNexus 支持的 DLP 接口版本（CONTRACT 第 3 节 9.8）
pub const API_VERSION: &str = "1";

/// `config.toml` 的 `[dlp]` 一节。不放进 `[[mcp.servers]]`，保证工具不会被登记成模型可用的能力
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DlpConfig {
    /// 启动命令，例如 `tools/dlp-egress/.venv/bin/dlp-egress-mcp`
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// 单次调用的超时（秒）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    5
}

/// DLP 不可用或拒绝时的错误。`Display` 是给用户看的大白话
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DlpError {
    /// 进程起不来、超时、返回结构不对、版本不对等：一律不发
    Unavailable(String),
}

impl std::fmt::Display for DlpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlpError::Unavailable(why) => {
                write!(f, "外发脱敏检查暂时不可用（{why}），这次不能发送")
            }
        }
    }
}

/// 一次外发评估的结果（只保留 AgentNexus 需要的字段；不含原文）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    /// ALLOW / ANONYMIZE / REVIEW / BLOCK
    pub decision: String,
    /// 只有为 true 才能发送，且只能发 `sanitized_text`
    pub ready_to_send: bool,
    pub sanitized_text: Option<String>,
    pub reason_codes: Vec<String>,
    /// 各类实体的个数（只记类型和个数）
    pub entity_counts: BTreeMap<String, usize>,
    pub token_count: usize,
    pub policy_version: String,
    pub request_id: String,
}

impl Evaluation {
    /// 不能发送时给用户的大白话说明
    pub fn refusal(&self) -> String {
        if self.reason_codes.iter().any(|c| c == "secret_blocked") {
            "内容里有密钥或凭证，已拦下，没有发送。如果这是真实的密钥，建议尽快更换。".into()
        } else if self.decision == "REVIEW" {
            "内容里有需要人工判断的机密信息，这次不发送。".into()
        } else if self.reason_codes.iter().any(|c| c == "destination_unknown") {
            "这个目的地还没有在外发脱敏工具里登记，不能发送。".into()
        } else {
            format!(
                "外发脱敏检查没有放行（原因：{}），这次不发送。",
                if self.reason_codes.is_empty() {
                    self.decision.clone()
                } else {
                    self.reason_codes.join("、")
                }
            )
        }
    }
}

/// DLP 客户端。连接按需建立
pub struct DlpClient {
    cfg: DlpConfig,
    tools: tokio::sync::Mutex<Option<ToolRegistry>>,
}

const TOOLS: [&str; 3] = ["egress_evaluate", "egress_restore", "egress_revoke"];

impl DlpClient {
    pub fn new(cfg: DlpConfig) -> Self {
        Self {
            cfg,
            tools: tokio::sync::Mutex::new(None),
        }
    }

    fn server_config(&self) -> McpServerConfig {
        McpServerConfig {
            id: "dlp".into(),
            transport: McpTransport::Stdio {
                command: self.cfg.command.clone(),
                args: self.cfg.args.clone(),
                cwd: None,
            },
            timeout_ms: self.cfg.timeout_secs.clamp(1, 300) * 1000,
            tools: TOOLS
                .iter()
                .map(|name| ToolBinding {
                    name: (*name).into(),
                    capability_id: format!("dlp.{name}"),
                    description: "外发脱敏（只由 AgentNexus 内部调用）".into(),
                    // evaluate 会写本机的映射库；三个工具都不对外写
                    effect: Effect::WorkspaceWrite,
                    input_types: vec!["application/json".into()],
                    output_types: vec!["application/json".into()],
                })
                .collect(),
        }
    }

    /// 调用一个工具，返回外壳里的 `data`。失败时丢掉连接，下次重新连接
    async fn call(&self, tool: &str, args: Value) -> Result<Value, DlpError> {
        let mut slot = self.tools.lock().await;
        if slot.is_none() {
            let registry = tokio::time::timeout(
                Duration::from_secs(self.cfg.timeout_secs.clamp(1, 300)),
                connect(&[self.server_config()]),
            )
            .await
            .map_err(|_| DlpError::Unavailable("启动超时".into()))?
            .map_err(|e| DlpError::Unavailable(format!("无法启动：{}", short(&e.to_string()))))?;
            *slot = Some(registry);
        }
        let handle = slot
            .as_ref()
            .and_then(|r| r.get(&format!("dlp.{tool}")))
            .ok_or_else(|| DlpError::Unavailable("缺少工具".into()))?;
        let result = handle.call(&args).await;
        let parsed = result
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("E_MCP_TIMEOUT") {
                    "没有在规定时间内回复".to_string()
                } else if msg.contains("E_MCP_OUTPUT") {
                    "返回结构不符合约定".to_string()
                } else {
                    format!("调用失败：{}", short(&msg))
                }
            })
            .and_then(|out| parse_envelope(&out.text));
        if parsed.is_err() {
            // 连接可能已经坏了：丢掉，下次重新连接
            *slot = None;
        }
        parsed.map_err(DlpError::Unavailable)
    }

    /// 外发前评估（只用 `text`，见 CONTRACT 第 1 节）
    pub async fn evaluate(
        &self,
        task_id: &str,
        channel: &str,
        destination_id: &str,
        purpose: &str,
        text: &str,
    ) -> Result<Evaluation, DlpError> {
        let data = self
            .call(
                "egress_evaluate",
                json!({
                    "task_id": task_id,
                    "channel": channel,
                    "destination_id": destination_id,
                    "purpose": purpose,
                    "text": text,
                }),
            )
            .await?;
        let evaluation = parse_evaluation(&data).map_err(DlpError::Unavailable)?;
        tracing::info!(
            event = "egress.evaluated",
            task_id = %task_id,
            channel,
            destination = %destination_id,
            purpose,
            decision = %evaluation.decision,
            ready_to_send = evaluation.ready_to_send,
            reasons = %evaluation.reason_codes.join(","),
            entities = ?evaluation.entity_counts,
            tokens = evaluation.token_count,
            policy_version = %evaluation.policy_version,
            request_id = %evaluation.request_id,
            text_len = text.chars().count(),
            "外发脱敏评估"
        );
        Ok(evaluation)
    }

    /// 把文本里的占位符还原成原文。**只用于给用户看的内容**（context 固定为 `local_ui`）
    pub async fn restore(&self, task_id: &str, text: &str) -> Result<String, DlpError> {
        let data = self
            .call(
                "egress_restore",
                json!({"task_id": task_id, "text": text, "context": "local_ui"}),
            )
            .await?;
        data["text"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| DlpError::Unavailable("还原结果缺少 text".into()))
    }

    /// 撤销这个任务的全部占位符映射。失败只记日志，不影响界面
    pub async fn revoke(&self, task_id: &str) {
        match self
            .call("egress_revoke", json!({"task_id": task_id}))
            .await
        {
            Ok(data) => {
                tracing::info!(event = "egress.revoked", task_id = %task_id, revoked = %data["revoked"], "已撤销占位符映射")
            }
            Err(e) => {
                tracing::warn!(event = "egress.revoke_failed", task_id = %task_id, error = %e, "撤销占位符映射失败")
            }
        }
    }
}

/// 只保留前 120 个字符，避免把长错误原样展示
fn short(s: &str) -> String {
    s.chars().take(120).collect()
}

/// 解析网关返回的文本：`{"status":"success","data":<CallToolResult>}`，
/// 再取 `structuredContent` 里 DLP 的统一外壳，检查 status 和 api_version
fn parse_envelope(gateway_text: &str) -> Result<Value, String> {
    let outer: Value =
        serde_json::from_str(gateway_text).map_err(|_| "返回结构不符合约定".to_string())?;
    let env = &outer["data"]["structuredContent"];
    if !env.is_object() {
        return Err("返回结构不符合约定".into());
    }
    if env["api_version"].as_str() != Some(API_VERSION) {
        return Err(format!(
            "接口版本不匹配（需要 {API_VERSION}，收到 {}）",
            env["api_version"]
        ));
    }
    match env["status"].as_str() {
        Some("success") if env["data"].is_object() => Ok(env["data"].clone()),
        Some("error") => Err(format!(
            "返回错误：{}",
            env["error"]["code"].as_str().unwrap_or("unknown")
        )),
        _ => Err("返回结构不符合约定".into()),
    }
}

fn parse_evaluation(data: &Value) -> Result<Evaluation, String> {
    let decision = data["decision"]
        .as_str()
        .filter(|d| ["ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"].contains(d))
        .ok_or("评估结果缺少合法的 decision")?
        .to_string();
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut entity_counts = BTreeMap::new();
    for e in data["entities"].as_array().into_iter().flatten() {
        if let Some(t) = e["type"].as_str() {
            *entity_counts.entry(t.to_string()).or_insert(0) += 1;
        }
    }
    let sanitized_text = data["sanitized_text"].as_str().map(String::from);
    // 只有 ALLOW / ANONYMIZE 且明确 ready_to_send、并且给了脱敏文本时才算能发
    let ready_to_send = data["ready_to_send"] == true
        && matches!(decision.as_str(), "ALLOW" | "ANONYMIZE")
        && sanitized_text.is_some();
    Ok(Evaluation {
        decision,
        ready_to_send,
        sanitized_text,
        reason_codes: strings(&data["reason_codes"]),
        entity_counts,
        token_count: data["tokens"].as_array().map(Vec::len).unwrap_or(0),
        policy_version: data["policy_version"].as_str().unwrap_or("").to_string(),
        request_id: data["request_id"].as_str().unwrap_or("").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(env: Value) -> String {
        json!({"status": "success", "data": {"structuredContent": env}}).to_string()
    }

    #[test]
    fn envelope_requires_version_and_success() {
        let ok = wrap(json!({"status":"success","api_version":"1","data":{"x":1},"error":null}));
        assert_eq!(parse_envelope(&ok).unwrap()["x"], 1);
        let v2 = wrap(json!({"status":"success","api_version":"2","data":{},"error":null}));
        assert!(parse_envelope(&v2).unwrap_err().contains("版本"));
        let err = wrap(
            json!({"status":"error","api_version":"1","data":null,"error":{"code":"restore_disabled"}}),
        );
        assert!(parse_envelope(&err)
            .unwrap_err()
            .contains("restore_disabled"));
        assert!(parse_envelope("not json").is_err());
        assert!(parse_envelope(&json!({"data":{}}).to_string()).is_err());
    }

    #[test]
    fn evaluation_ready_only_for_allow_or_anonymize_with_text() {
        let e = parse_evaluation(
            &json!({"decision":"ANONYMIZE","ready_to_send":true,"sanitized_text":"x",
            "entities":[{"type":"CUSTOMER"},{"type":"CUSTOMER"}],"tokens":["[CUSTOMER_001]"]}),
        )
        .unwrap();
        assert!(e.ready_to_send);
        assert_eq!(e.entity_counts["CUSTOMER"], 2);
        // REVIEW 即使声称 ready 也不能发
        let r = parse_evaluation(
            &json!({"decision":"REVIEW","ready_to_send":true,"sanitized_text":"x"}),
        )
        .unwrap();
        assert!(!r.ready_to_send);
        assert!(r.refusal().contains("人工"));
        // 没有脱敏文本也不能发
        let n = parse_evaluation(&json!({"decision":"ALLOW","ready_to_send":true})).unwrap();
        assert!(!n.ready_to_send);
        let s = parse_evaluation(
            &json!({"decision":"BLOCK","ready_to_send":false,"reason_codes":["secret_blocked"]}),
        )
        .unwrap();
        assert!(s.refusal().contains("密钥") && s.refusal().contains("更换"));
        assert!(parse_evaluation(&json!({"decision":"MAYBE"})).is_err());
    }
}
