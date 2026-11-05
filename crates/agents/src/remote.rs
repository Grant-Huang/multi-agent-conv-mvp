//! 远程节点上的 agent。
//!
//! 节点（`an-node`）运行在另一台机器上，负责在那台机器本地启动 agent、收集 git diff、执行权限规则。
//! AgentNexus 通过一个很小的 HTTP 协议和节点通信：
//!
//! | 接口 | 认证 | 用途 |
//! |---|---|---|
//! | `GET  /node/info`   | 无     | 节点身份（发现和配对前确认） |
//! | `POST /node/pair`   | 配对码 | 换取长期令牌 |
//! | `GET  /node/agents` | 令牌   | 节点上的 agent 列表（同时用作心跳） |
//! | `POST /node/run`    | 令牌   | 执行任务，响应是 SSE：`progress`* → `result` 或 `error` |
//!
//! 取消：AgentNexus 取消任务时断开 `/node/run` 连接，节点检测到后终止本地 agent。

use std::time::Duration;

use an_core::node::{NodeError, NodeInfo, PairRequest, PairResponse, Progress, RunRequest};
use an_core::{codes, AgentAdapter, AgentCard, AgentError, EventSink, TaskResult, TaskSpec};
use anyhow::Result;
use futures::StreamExt;
use reqwest::StatusCode;

/// 与某个节点的连接信息（配对后保存在 AgentNexus 的数据库中）
#[derive(Debug, Clone)]
pub struct NodeConn {
    pub node_id: String,
    pub name: String,
    /// 例如 `http://192.168.1.23:7788`
    pub address: String,
    pub token: String,
}

/// 访问节点的 HTTP 客户端
#[derive(Clone)]
pub struct NodeClient {
    http: reqwest::Client,
}

impl Default for NodeClient {
    fn default() -> Self {
        // 只限制连接超时；/node/run 是长连接，不能设整体超时（任务超时由编排层控制）
        let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build().expect("创建 HTTP 客户端失败");
        Self { http }
    }
}

/// 把节点返回的错误码映射到本地常量（未知错误码归为协议错误）
fn known_code(code: &str) -> &'static str {
    const ALL: &[&str] = &[
        codes::AGENT_NOT_INSTALLED,
        codes::AGENT_SPAWN,
        codes::AGENT_TIMEOUT,
        codes::AGENT_RUNTIME,
        codes::ACP_HANDSHAKE,
        codes::ACP_RUNTIME,
        codes::ACP_STOP,
        codes::NODE_AUTH,
        codes::NODE_PAIR,
        codes::NODE_BUSY,
        codes::NODE_AGENT_NOT_FOUND,
    ];
    ALL.iter().find(|c| **c == code).copied().unwrap_or(codes::NODE_PROTOCOL)
}

/// 把非 2xx 响应转换成带错误码的错误
async fn http_error(resp: reqwest::Response, what: &str) -> AgentError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let parsed: Option<NodeError> = serde_json::from_str(&body).ok();
    let msg = parsed.as_ref().map(|e| e.message.clone()).unwrap_or_else(|| an_core::truncate_chars(&body, 300));
    let code = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => codes::NODE_AUTH,
        _ => parsed.map(|e| known_code(&e.code)).unwrap_or(codes::NODE_PROTOCOL),
    };
    AgentError::new(code, format!("{what}：HTTP {status} {msg}"))
}

impl NodeClient {
    fn url(address: &str, path: &str) -> String {
        format!("{}{}", address.trim_end_matches('/'), path)
    }

    fn unreachable(address: &str, e: reqwest::Error) -> AgentError {
        AgentError::new(codes::NODE_UNREACHABLE, format!("无法连接节点 {address}：{e}"))
    }

    /// 读取节点身份（无需认证）
    pub async fn info(&self, address: &str) -> Result<NodeInfo, AgentError> {
        let resp = self.http.get(Self::url(address, "/node/info")).timeout(Duration::from_secs(5)).send().await.map_err(|e| Self::unreachable(address, e))?;
        if !resp.status().is_success() {
            return Err(http_error(resp, "读取节点信息失败").await);
        }
        resp.json().await.map_err(|e| AgentError::new(codes::NODE_PROTOCOL, format!("节点信息格式错误：{e}")))
    }

    /// 用配对码换取令牌
    pub async fn pair(&self, address: &str, code: &str, client_name: &str) -> Result<PairResponse, AgentError> {
        let body = PairRequest { code: code.trim().into(), client_name: client_name.into() };
        let resp = self
            .http
            .post(Self::url(address, "/node/pair"))
            .timeout(Duration::from_secs(10))
            .json(&body)
            .send()
            .await
            .map_err(|e| Self::unreachable(address, e))?;
        if !resp.status().is_success() {
            let mut e = http_error(resp, "配对失败").await;
            if e.code == codes::NODE_AUTH || e.code == codes::NODE_PROTOCOL {
                e.code = codes::NODE_PAIR;
            }
            return Err(e);
        }
        resp.json().await.map_err(|e| AgentError::new(codes::NODE_PROTOCOL, format!("配对响应格式错误：{e}")))
    }

    /// 读取节点上的 agent 列表（也用作心跳）
    pub async fn agents(&self, conn: &NodeConn) -> Result<Vec<AgentCard>, AgentError> {
        let resp = self
            .http
            .get(Self::url(&conn.address, "/node/agents"))
            .bearer_auth(&conn.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| Self::unreachable(&conn.address, e))?;
        if !resp.status().is_success() {
            return Err(http_error(resp, "读取节点 agent 列表失败").await);
        }
        resp.json().await.map_err(|e| AgentError::new(codes::NODE_PROTOCOL, format!("agent 列表格式错误：{e}")))
    }
}

/// 节点名转成适合放进 agent id 的短名：小写、空白和特殊字符换成 `-`
pub fn node_slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    s.trim_matches('-').to_string()
}

/// 远程 agent：在 AgentNexus 看来和本机 agent 一样，只是执行发生在节点上
pub struct RemoteAgent {
    client: NodeClient,
    conn: NodeConn,
    /// 节点上的 agent id
    remote_id: String,
    card: AgentCard,
}

impl RemoteAgent {
    /// `remote`：节点报告的能力卡片；`online`：节点当前是否在线
    pub fn new(client: NodeClient, conn: NodeConn, remote: AgentCard, online: bool) -> Self {
        let enabled = online && remote.enabled;
        let disabled_reason = if !online {
            Some(format!("节点 {} 离线", conn.name))
        } else if !remote.enabled {
            remote.disabled_reason.clone()
        } else {
            None
        };
        let card = AgentCard {
            // 例如 hermes@mac-mini：同一个 agent 在不同机器上 id 不同，但 agent_type 相同
            id: format!("{}@{}", remote.id, node_slug(&conn.name)),
            kind: "node".into(),
            location: conn.name.clone(),
            name: format!("{}（{}）", remote.name, conn.name),
            enabled,
            disabled_reason,
            ..remote.clone()
        };
        Self { client, conn, remote_id: remote.id, card }
    }

    pub fn node_id(&self) -> &str {
        &self.conn.node_id
    }
}

#[async_trait::async_trait]
impl AgentAdapter for RemoteAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        let address = &self.conn.address;
        tracing::info!(event = "node.run_start", node_id = %self.conn.node_id, agent_id = %self.card.id, task_id = %task.task_id);
        events.progress(format!("发送到节点 {}（{}）", self.conn.name, address));
        let task_id = task.task_id.clone();
        let body = RunRequest { agent_id: self.remote_id.clone(), task };
        let resp = self
            .client
            .http
            .post(NodeClient::url(address, "/node/run"))
            .bearer_auth(&self.conn.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| NodeClient::unreachable(address, e))?;
        if !resp.status().is_success() {
            return Err(http_error(resp, "节点拒绝执行").await.into());
        }

        // 解析 SSE：事件之间以空行分隔，每个事件有 `event:` 和 `data:` 行
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AgentError::new(codes::NODE_UNREACHABLE, format!("与节点的连接中断：{e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buf.find("\n\n") {
                let frame: String = buf.drain(..end + 2).collect();
                let (mut name, mut data) = (String::new(), String::new());
                for line in frame.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        name = v.trim().to_string();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim_start());
                    }
                }
                match name.as_str() {
                    "progress" => {
                        if let Ok(p) = serde_json::from_str::<Progress>(&data) {
                            events.progress(p.message);
                        }
                    }
                    "result" => {
                        let r: TaskResult = serde_json::from_str(&data)
                            .map_err(|e| AgentError::new(codes::NODE_PROTOCOL, format!("节点返回的结果格式错误：{e}")))?;
                        tracing::info!(event = "node.run_end", node_id = %self.conn.node_id, task_id = %task_id, agent_id = %self.card.id, status = ?r.status);
                        return Ok(r);
                    }
                    "error" => {
                        let e: NodeError = serde_json::from_str(&data).unwrap_or(NodeError { code: codes::NODE_PROTOCOL.into(), message: data.clone() });
                        return Err(AgentError::new(known_code(&e.code), format!("节点 {}：{}", self.conn.name, e.message)).into());
                    }
                    _ => {} // 心跳等其他事件
                }
            }
        }
        Err(AgentError::new(codes::NODE_UNREACHABLE, format!("节点 {} 在返回结果前断开了连接", self.conn.name)).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug() {
        assert_eq!(node_slug("Mac-mini"), "mac-mini");
        assert_eq!(node_slug("Jacer's MacBook Pro"), "jacer-s-macbook-pro");
        assert_eq!(node_slug("办公室电脑"), "办公室电脑");
    }

    #[test]
    fn codes_map() {
        assert_eq!(known_code("E_NODE_BUSY"), codes::NODE_BUSY);
        assert_eq!(known_code("E_WHATEVER"), codes::NODE_PROTOCOL);
    }
}
