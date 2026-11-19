//! 阶段⑤a-2：AgentNexus 侧的模拟工厂连接器（设计文档 §11a，"⑤a-2 设计"，决策 DES-27）。
//!
//! 参照 RelOS 接入模式（`mom_relos`），在服务层异步调用模拟工厂（`an-des-mcp`，stdio），引擎保持确定、可重放：
//! - `tick`：逐分钟推进模拟工厂 → 拉取事件与可观测快照 → 交给引擎 → 推进引擎；
//! - `dispatch_writes`：把引擎中审批后产生的写操作带幂等键与审批引用发给模拟工厂，回执写回动作；
//! - `agent_tools` / `call_agent_tool`：只向 agent 与桌面开放观测类工具，其余在本地拒绝，不发给模拟工厂；
//! - 模拟工厂不在线（命令不存在、进程退出、协议错误、超时）时返回 `E_DES_UNAVAILABLE`，引擎状态不变，
//!   不回退到进程内仿真，也不写入假观测；
//! - 管理口令只由连接器持有（经环境变量传给子进程），用于读取标准答案交给审核层，不进入任何 agent 上下文。
//!
//! 超时、取消与清理：
//! - 握手 10 秒、每次工具调用 30 秒超时；
//! - 子进程设置 `kill_on_drop`，连接器被丢弃时一并结束；
//! - 引擎锁只在同步调用引擎时短暂持有，等待模拟工厂时不持锁。

use an_des::{tool_catalog, ToolCategory};
use an_runtime::mom::des::remote::RemoteWrite;
use an_runtime::mom::des::DesSimulationConfig;
use an_runtime::mom::{Engine, EventInput};
use anyhow::{anyhow, bail, Context, Result};
use rmcp::{
    model::{CallToolRequestParams, PaginatedRequestParams},
    service::RunningService,
    transport::async_rw::AsyncRwTransport,
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::{Child, Command};

/// 模拟工厂不可用（命令不存在、进程退出、协议错误、超时）时的错误码。
pub const E_DES_UNAVAILABLE: &str = "E_DES_UNAVAILABLE";
/// 管理口令所在的环境变量（与 `an-des-mcp` 一致）。
const ADMIN_TOKEN_ENV: &str = "AN_DES_ADMIN_TOKEN";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// 每次拉取事件的条数。
const POLL_LIMIT: u64 = 1000;

fn unavailable(detail: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("{E_DES_UNAVAILABLE}: {detail}")
}

fn lock(engine: &Mutex<Engine>) -> Result<std::sync::MutexGuard<'_, Engine>> {
    engine.lock().map_err(|_| anyhow!("runtime unavailable"))
}

/// 一个仿真的连接状态：事件游标、场景起点（起点之前模拟工厂没有时钟）。
#[derive(Clone, Debug)]
struct SimState {
    cursor: u64,
    start_minute: u64,
}

/// 连接到一个模拟工厂进程。
pub struct DesConnector {
    client: RunningService<RoleClient, ()>,
    child: Child,
    admin_token: String,
    /// 开放给 agent 的工具（观测类，且模拟工厂确实提供）。
    agent_tools: BTreeSet<String>,
    sims: Mutex<BTreeMap<String, SimState>>,
    sent: Mutex<Vec<Value>>,
}

impl DesConnector {
    /// 启动模拟工厂（stdio）并完成握手；`admin_token` 通过环境变量 `AN_DES_ADMIN_TOKEN` 传给它。
    pub async fn spawn(command: &Path, args: &[String], admin_token: &str) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .env(ADMIN_TOKEN_ENV, admin_token)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| unavailable(format!("cannot start {}: {e}", command.display())))?;
        let stdout = child.stdout.take().context("factory stdout")?;
        let stdin = child.stdin.take().context("factory stdin")?;
        let client = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            ().serve(AsyncRwTransport::new(stdout, stdin)),
        )
        .await
        .map_err(|_| unavailable("handshake timed out"))?
        .map_err(|e| unavailable(format!("handshake failed: {e}")))?;
        // 开放给 agent 的工具：模拟工厂实际提供的工具中，目录里属于观测类的。
        let offered: BTreeSet<String> = {
            let mut names = BTreeSet::new();
            let mut cursor = None;
            for _ in 0..32 {
                let page = tokio::time::timeout(
                    CALL_TIMEOUT,
                    client.list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor))),
                )
                .await
                .map_err(|_| unavailable("tools/list timed out"))?
                .map_err(|e| unavailable(format!("tools/list failed: {e}")))?;
                names.extend(page.tools.into_iter().map(|t| t.name.to_string()));
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            names
        };
        let agent_tools = tool_catalog()
            .into_iter()
            .filter(|t| t.category == ToolCategory::Observe && offered.contains(&t.name))
            .map(|t| t.name)
            .collect();
        Ok(Self {
            client,
            child,
            admin_token: admin_token.to_string(),
            agent_tools,
            sims: Mutex::new(BTreeMap::new()),
            sent: Mutex::new(vec![]),
        })
    }

    /// 模拟工厂进程编号（测试用：模拟进程意外退出）。
    pub fn child_pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// 调用模拟工厂的工具：传输层失败报 `E_DES_UNAVAILABLE`；业务错误（工具错误）照原文返回。
    async fn call(&self, tool: &str, args: Value) -> Result<Value> {
        let args = args
            .as_object()
            .cloned()
            .context("tool arguments must be an object")?;
        let result = tokio::time::timeout(
            CALL_TIMEOUT,
            self.client
                .peer()
                .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args)),
        )
        .await
        .map_err(|_| unavailable(format!("{tool} timed out")))?
        .map_err(|e| unavailable(format!("{tool}: {e}")))?;
        if result.is_error == Some(true) {
            let text = serde_json::to_value(&result.content)
                .ok()
                .and_then(|c| c[0]["text"].as_str().map(String::from))
                .unwrap_or_default();
            bail!("factory rejected {tool}: {text}");
        }
        result
            .structured_content
            .ok_or_else(|| unavailable(format!("{tool} returned no structured content")))
    }

    /// 在模拟工厂中创建仿真，并在引擎中登记（远程模式），同时取回标准答案交给审核层。
    /// 起点取配置起点与引擎时钟中较晚者（与进程内创建一致）。
    pub async fn create_simulation(
        &self,
        engine: &Mutex<Engine>,
        mut config: DesSimulationConfig,
    ) -> Result<String> {
        config.scenario.start_minute = config.scenario.start_minute.max(lock(engine)?.now());
        let created = self
            .call("simulation_create", serde_json::to_value(&config)?)
            .await?;
        let id = created["simulation_id"]
            .as_str()
            .context("factory returned no simulation id")?
            .to_string();
        let start_minute = config.scenario.start_minute;
        lock(engine)?.create_remote_des_simulation(&id, config)?;
        self.refresh_truth(engine, &id).await?;
        self.sims
            .lock()
            .map_err(|_| anyhow!("connector state"))?
            .insert(
                id.clone(),
                SimState {
                    cursor: 0,
                    start_minute,
                },
            );
        Ok(id)
    }

    /// 从管理入口读取标准答案交给引擎（只供审核层）。
    async fn refresh_truth(&self, engine: &Mutex<Engine>, sim: &str) -> Result<()> {
        let truth = self
            .call(
                "simulation_ground_truth",
                json!({"simulation_id": sim, "admin_token": self.admin_token}),
            )
            .await?;
        lock(engine)?.des_set_remote_truth(sim, truth)
    }

    /// 逐分钟推进到 `to_minute`：模拟工厂推进 → 拉取事件与快照 → 交给引擎 → 引擎推进。
    /// 任何一步失败都在交给引擎之前返回，引擎状态不变。
    pub async fn tick(&self, engine: &Mutex<Engine>, to_minute: u64) -> Result<()> {
        let sims: Vec<(String, SimState)> = self
            .sims
            .lock()
            .map_err(|_| anyhow!("connector state"))?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let now = lock(engine)?.now();
        // 所有仿真的起点之前，模拟工厂没有时钟：引擎直接推进到起点前一分钟。
        let first_start = sims.iter().map(|(_, s)| s.start_minute).min();
        let mut minute = now;
        if let Some(first) = first_start {
            if minute + 1 < first {
                let to = (first - 1).min(to_minute);
                lock(engine)?.advance(to)?;
                minute = to;
            }
        }
        while minute <= to_minute {
            let mut batches = vec![];
            for (id, state) in &sims {
                if minute < state.start_minute {
                    continue;
                }
                self.call(
                    "simulation_advance",
                    json!({"simulation_id": id, "to_minute": minute}),
                )
                .await?;
                let mut cursor = self
                    .sims
                    .lock()
                    .map_err(|_| anyhow!("connector state"))?
                    .get(id)
                    .map(|s| s.cursor)
                    .unwrap_or(0);
                let mut events: Vec<EventInput> = vec![];
                loop {
                    let page = self
                        .call(
                            "events_poll",
                            json!({"simulation_id": id, "cursor": cursor, "limit": POLL_LIMIT}),
                        )
                        .await?;
                    let batch: Vec<EventInput> =
                        serde_json::from_value(page["events"].clone()).context("factory events")?;
                    let next = page["next_cursor"].as_u64().context("next_cursor")?;
                    let done = batch.is_empty();
                    events.extend(batch);
                    cursor = next;
                    if done {
                        break;
                    }
                }
                let observation = self
                    .call("equipment_status", json!({"simulation_id": id}))
                    .await?;
                batches.push((id.clone(), events, observation, cursor));
            }
            // 全部仿真都取到之后才交给引擎并推进，避免半途失败留下不一致的状态。
            {
                let mut e = lock(engine)?;
                for (id, events, observation, _) in &batches {
                    e.des_ingest_remote(id, events.clone(), observation.clone())?;
                }
                e.advance(minute)?;
            }
            let mut state = self.sims.lock().map_err(|_| anyhow!("connector state"))?;
            for (id, _, _, cursor) in batches {
                if let Some(s) = state.get_mut(&id) {
                    s.cursor = cursor;
                }
            }
            drop(state);
            minute += 1;
        }
        Ok(())
    }

    /// 发出引擎中待发的写操作，回执写回动作，并刷新受影响仿真的标准答案。返回本次收到的回执。
    pub async fn dispatch_writes(&self, engine: &Mutex<Engine>) -> Result<Vec<Value>> {
        let pending = lock(engine)?.des_pending_writes();
        if pending.is_empty() {
            return Ok(vec![]);
        }
        let receipts = self.send_writes(&pending).await?;
        let mut touched = BTreeSet::new();
        {
            let mut e = lock(engine)?;
            for (w, receipt) in pending.iter().zip(&receipts) {
                e.des_record_write_receipt(&w.key, receipt.clone())?;
                if let Some(sim) = w.args["simulation_id"].as_str() {
                    touched.insert(sim.to_string());
                }
            }
        }
        // 干预改变了之后的世界，标准答案随之更新（与进程内重新推演后一致）。
        for sim in touched {
            self.refresh_truth(engine, &sim).await?;
        }
        Ok(receipts)
    }

    /// 发送写操作并返回回执（同键重发时模拟工厂返回原回执），不记录到引擎。
    pub async fn send_writes(&self, writes: &[RemoteWrite]) -> Result<Vec<Value>> {
        let mut receipts = vec![];
        for w in writes {
            let receipt = self.call(&w.tool, w.args.clone()).await?;
            self.sent
                .lock()
                .map_err(|_| anyhow!("connector state"))?
                .push(json!({"tool": w.tool, "args": w.args, "receipt": receipt}));
            receipts.push(receipt);
        }
        Ok(receipts)
    }

    /// 开放给 agent 与桌面的工具名（只有观测类）。
    pub fn agent_tools(&self) -> Vec<String> {
        self.agent_tools.iter().cloned().collect()
    }

    /// 以 agent 身份调用工具：不在 `agent_tools` 中的一律拒绝，不发给模拟工厂。
    pub async fn call_agent_tool(&self, tool: &str, args: Value) -> Result<Value> {
        if !self.agent_tools.contains(tool) {
            bail!("tool {tool} is not allowed for agents; only observation tools are exposed");
        }
        self.call(tool, args).await
    }

    /// 已发出的写操作记录（工具名、参数、回执）。
    pub fn sent_writes(&self) -> Vec<Value> {
        self.sent.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
