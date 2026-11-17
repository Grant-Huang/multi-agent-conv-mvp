//! 托管配置、热注册与撤销（AG.2）。
//!
//! - 托管文件 `agents.managed.toml` 放在数据库同目录，由系统写入；用户手写的 `config.toml` 永远优先。
//! - 修改分两步：**提议**（只校验、不写入，返回"将会发生什么"和一次性确认凭证）→ **确认**（带凭证才写入）。
//!   凭证只返回给发起提议的界面；系统里的模型拿不到它，所以模型说"同意"不算数。
//!   边界：挡不住本机其他能访问 HTTP 接口的程序（与"批准任务"接口相同，系统没有登录鉴权，只监听本机）。
//! - 写入：同一时间只处理一个修改；先写临时文件再改名替换；修改前把当前版本存进历史（最多 10 个），可连续撤销。
//! - 生效：按"config.toml → 托管 → 自动发现的预设"重新计算应注册的 ACP agent，与注册表对比后增删，不重启。
//!
//! 锁顺序：确认锁（tokio）→ 状态锁（std）→ 注册表锁；注册表锁内不做 I/O。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_agents::acp::{presets, AcpConfig, PROBE_TIMEOUT_SECS};
use an_agents::AcpAgent;
use an_core::{codes, AgentAdapter, AgentCard, ProbeOutcome, Role};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::registry::{check_group, role_label, AgentRegistry};
use crate::{Prober, ProberSet};

/// 历史版本最多保留几个
pub const HISTORY_KEEP: usize = 10;
/// 提议的默认有效期
pub const DEFAULT_PROPOSAL_TTL: Duration = Duration::from_secs(10 * 60);
/// 托管文件名（与数据库同目录）
pub const MANAGED_FILE: &str = "agents.managed.toml";

// ───────────────────────── 托管文件格式 ─────────────────────────

/// 托管文件的内容。字段写法与 config.toml 相同，只多一个 `version`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedConfig {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub acp: ManagedAcp,
    #[serde(default)]
    pub agents: ManagedAgents,
}

impl Default for ManagedConfig {
    fn default() -> Self {
        Self {
            version: 1,
            acp: ManagedAcp::default(),
            agents: ManagedAgents::default(),
        }
    }
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagedAcp {
    /// 停用：这些 id 的托管 agent 和自动发现的预设不注册
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
    /// 托管的 ACP agent（字段与 config.toml 的 [[acp.agents]] 相同）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<AcpConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManagedAgents {
    /// 托管的执行组（只用于 config.toml 没写的角色）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assign: BTreeMap<String, Vec<String>>,
}

impl ManagedConfig {
    fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| "version = 1\n".into())
    }
}

// ───────────────────────── 提议与错误 ─────────────────────────

/// 一个修改请求（HTTP 请求体，`kind` 区分类型）
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeRequest {
    /// 接入目录里的预设
    AddPreset { id: String },
    /// 接入自定义 ACP agent（专业模式）
    AddCustom { agent: AcpConfig },
    /// 设置某角色的默认执行者（执行组）
    SetDefault { role: Role, ids: Vec<String> },
    /// 停用托管的或自动发现的 agent
    Disable { id: String },
    /// 恢复启用
    Enable { id: String },
    /// 移除托管的自定义 agent
    Remove { id: String },
    /// 撤销最近一次修改
    Revert,
}

impl ChangeRequest {
    fn kind(&self) -> &'static str {
        match self {
            Self::AddPreset { .. } => "add_preset",
            Self::AddCustom { .. } => "add_custom",
            Self::SetDefault { .. } => "set_default",
            Self::Disable { .. } => "disable",
            Self::Enable { .. } => "enable",
            Self::Remove { .. } => "remove",
            Self::Revert => "revert",
        }
    }
}

/// 提议的结果（返回给界面）
#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub id: String,
    pub kind: String,
    /// 给用户看的"将会发生什么"
    pub summary: String,
    pub warnings: Vec<String>,
    /// 修改前后的托管文件内容（专业模式展开查看）
    pub before: String,
    pub after: String,
    /// 一次性确认凭证：只给发起提议的界面
    pub token: String,
    pub expires_in_secs: u64,
}

/// 确认后的结果
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub kind: String,
    pub summary: String,
    /// 这次新接入的 agent 的握手检测结果
    pub probes: Vec<ProbeOutcome>,
}

/// 错误：HTTP 层按类型转换状态码
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// 404：提议或 agent 不存在
    NotFound(String),
    /// 403：凭证错误
    BadToken,
    /// 410：提议已过期
    Expired,
    /// 409：与当前状态冲突（config.toml 已写、没有可撤销的版本、托管文件损坏等）
    Conflict(String),
    /// 422：请求本身不合格
    Invalid(String),
    /// 500：写文件失败等
    Internal(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) | Self::Conflict(m) | Self::Invalid(m) | Self::Internal(m) => {
                f.write_str(m)
            }
            Self::BadToken => f.write_str("确认凭证不对，这次修改没有生效"),
            Self::Expired => f.write_str("这个提议已经过期，请重新发起"),
        }
    }
}

// ───────────────────────── 管理器 ─────────────────────────

struct Pending {
    change: ChangeRequest,
    token: String,
    expires: Instant,
}

struct State {
    managed: ManagedConfig,
    load_error: Option<String>,
    proposals: HashMap<String, Pending>,
    /// 当前由"托管 / 自动发现"注册的 agent：id → 注册时的配置（用来判断是否需要重新注册）
    dynamic: BTreeMap<String, AcpConfig>,
    ttl: Duration,
}

/// 托管配置管理器
pub struct AgentConfigManager {
    /// 托管文件路径；数据库在内存里（`:memory:`）时为 None，不支持修改
    path: Option<PathBuf>,
    state: Mutex<State>,
    /// 确认串行化（写文件、更新注册表都在里面）
    apply_lock: tokio::sync::Mutex<()>,
}

impl Default for AgentConfigManager {
    fn default() -> Self {
        Self::with_state(None, ManagedConfig::default(), None)
    }
}

impl AgentConfigManager {
    fn with_state(
        path: Option<PathBuf>,
        managed: ManagedConfig,
        load_error: Option<String>,
    ) -> Self {
        Self {
            path,
            state: Mutex::new(State {
                managed,
                load_error,
                proposals: HashMap::new(),
                dynamic: BTreeMap::new(),
                ttl: DEFAULT_PROPOSAL_TTL,
            }),
            apply_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// 启动时读取托管文件。读不出来或格式错误：不报错退出，忽略其中内容并记录原因，**不覆盖坏文件**。
    pub fn load(config: &AppConfig) -> Self {
        let Some(path) = managed_path(config) else {
            return Self::default();
        };
        if !path.exists() {
            return Self::with_state(Some(path), ManagedConfig::default(), None);
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<ManagedConfig>(&t).map_err(|e| e.to_string()))
        {
            Ok(m) => Self::with_state(Some(path), m, None),
            Err(e) => {
                let msg = format!(
                    "[{}] 托管配置文件 {} 读取失败：{e}。已忽略其中的内容；修好或删掉这个文件后重启",
                    codes::AGENT_MANAGED_INVALID,
                    path.display()
                );
                tracing::error!(event = "agent.managed_invalid", code = codes::AGENT_MANAGED_INVALID, path = %path.display(), error = %e, "托管配置文件读取失败，已忽略");
                Self::with_state(Some(path), ManagedConfig::default(), Some(msg))
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 当前托管内容
    pub fn managed(&self) -> ManagedConfig {
        self.lock().managed.clone()
    }

    /// 托管文件读取失败的原因（含错误码）
    pub fn load_error(&self) -> Option<String> {
        self.lock().load_error.clone()
    }

    /// 调整提议的有效期（测试用）
    pub fn set_proposal_ttl(&self, ttl: Duration) {
        self.lock().ttl = ttl;
    }

    /// 启动时登记由"托管 / 自动发现"注册的 agent
    pub(crate) fn set_dynamic(&self, dynamic: BTreeMap<String, AcpConfig>) {
        self.lock().dynamic = dynamic;
    }

    fn history_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .map(|p| p.with_file_name("agents.managed.history"))
    }

    /// 历史版本文件（按编号从旧到新）
    fn history_files(&self) -> Vec<PathBuf> {
        let Some(dir) = self.history_dir() else {
            return vec![];
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    fn history_top(&self) -> Option<ManagedConfig> {
        let last = self.history_files().pop()?;
        std::fs::read_to_string(last)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
    }

    /// 把当前版本存进历史，只保留最新的 HISTORY_KEEP 个
    fn push_history(&self, current: &ManagedConfig) -> Result<(), ConfigError> {
        let dir = self.history_dir().ok_or_else(no_data_dir)?;
        std::fs::create_dir_all(&dir).map_err(|e| ConfigError::Internal(e.to_string()))?;
        let next = self
            .history_files()
            .last()
            .and_then(|p| p.file_stem()?.to_str()?.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        write_atomic(&dir.join(format!("{next:08}.toml")), &current.to_toml())?;
        let files = self.history_files();
        if files.len() > HISTORY_KEEP {
            for old in &files[..files.len() - HISTORY_KEEP] {
                let _ = std::fs::remove_file(old);
            }
        }
        Ok(())
    }

    fn pop_history(&self) {
        if let Some(last) = self.history_files().pop() {
            let _ = std::fs::remove_file(last);
        }
    }
}

fn no_data_dir() -> ConfigError {
    ConfigError::Conflict("数据库在内存里，没有可写的数据目录，不能保存托管配置".into())
}

/// 托管文件路径：数据库同目录。数据库在内存里时不支持。
pub fn managed_path(config: &AppConfig) -> Option<PathBuf> {
    let db = Path::new(&config.server.db_path);
    if db == Path::new(":memory:") {
        return None;
    }
    Some(
        db.parent()
            .map(|d| d.join(MANAGED_FILE))
            .unwrap_or_else(|| PathBuf::from(MANAGED_FILE)),
    )
}

/// 先写临时文件、刷盘，再改名替换：中途出错不会留下半截文件
fn write_atomic(path: &Path, text: &str) -> Result<(), ConfigError> {
    use std::io::Write;
    let err =
        |e: std::io::Error| ConfigError::Internal(format!("写入 {} 失败：{e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).map_err(err)?;
        f.write_all(text.as_bytes()).map_err(err)?;
        f.sync_all().map_err(err)?;
    }
    std::fs::rename(&tmp, path).map_err(err)
}

// ───────────────────────── 计算"应注册什么" ─────────────────────────

/// 环境变量只允许写成 `${变量名}` 的引用
fn is_env_reference(v: &str) -> bool {
    v.strip_prefix("${")
        .and_then(|s| s.strip_suffix('}'))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// 注册时把 `${VAR}` 换成当前进程的环境变量值（值只在内存里，不写进文件）
pub(crate) fn resolve_env(cfg: &AcpConfig) -> AcpConfig {
    let mut c = cfg.clone();
    for (k, v) in c.env.iter_mut() {
        if is_env_reference(v) {
            let name = &v[2..v.len() - 1];
            match std::env::var(name) {
                Ok(val) => *v = val,
                Err(_) => {
                    tracing::warn!(event = "agent.env_missing", agent_id = %cfg.id, key = %k, var = %name, "托管 agent 引用的环境变量没有设置");
                    v.clear();
                }
            }
        }
    }
    c
}

/// 应由"托管 / 自动发现"注册的 agent（按注册顺序，同 id 先到先得）。
/// config.toml 的 [[acp.agents]] 不在这里：它们启动时注册，对话里不改。
pub(crate) fn dynamic_desired(config: &AppConfig, managed: &ManagedConfig) -> Vec<AcpConfig> {
    let taken: Vec<&str> = config.acp.agents.iter().map(|a| a.id.as_str()).collect();
    let disabled = &managed.acp.disabled;
    let mut out: Vec<AcpConfig> = vec![];
    let free = |id: &str, out: &Vec<AcpConfig>| {
        !taken.contains(&id) && !disabled.iter().any(|d| d == id) && !out.iter().any(|a| a.id == id)
    };
    for a in &managed.acp.agents {
        if free(&a.id, &out) {
            out.push(a.clone());
        }
    }
    if config.acp.auto_detect {
        for p in presets() {
            if free(&p.id, &out) && an_agents::util::resolve_command(&p.command).is_some() {
                out.push(p);
            }
        }
    }
    out
}

/// 某个角色的默认工作目录（与启动时注册的规则相同）
pub(crate) fn default_workdir(config: &AppConfig, role: Role) -> PathBuf {
    match role {
        Role::Coding => config.agents.coding.cfg.workdir.clone(),
        _ => config.agents.general.workspace_dir.clone(),
    }
}

/// 合并执行组：config.toml 写了的角色以它为准，托管的只补没写的角色
pub fn merged_assign(
    config_assign: &BTreeMap<String, Vec<String>>,
    managed: &ManagedConfig,
) -> BTreeMap<String, Vec<String>> {
    let mut m = config_assign.clone();
    for (role, ids) in &managed.agents.assign {
        m.entry(role.clone()).or_insert_with(|| ids.clone());
    }
    m
}

fn same_config(a: &AcpConfig, b: &AcpConfig) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

// ───────────────────────── 运行时入口 ─────────────────────────

/// 提议和确认需要的运行时部件
pub struct Ctx<'a> {
    pub config: &'a AppConfig,
    pub registry: &'a Arc<AgentRegistry>,
    pub probers: &'a ProberSet,
}

impl Ctx<'_> {
    /// 修改生效后将会注册的全部 agent 卡片：当前卡片去掉"托管 / 自动发现"的，再加上新计算出的
    fn preview_cards(
        &self,
        dynamic_now: &BTreeMap<String, AcpConfig>,
        desired: &[AcpConfig],
    ) -> Vec<AgentCard> {
        let mut cards: Vec<AgentCard> = self
            .registry
            .cards()
            .into_iter()
            .filter(|c| !dynamic_now.contains_key(&c.id))
            .collect();
        for cfg in desired {
            if cards.iter().any(|c| c.id == cfg.id) {
                continue;
            }
            let agent = AcpAgent::new(cfg.clone(), default_workdir(self.config, cfg.role));
            cards.push(agent.card().clone());
        }
        cards
    }
}

impl AgentConfigManager {
    /// 提议：只校验、不写入
    pub fn propose(&self, ctx: &Ctx<'_>, change: ChangeRequest) -> Result<Proposal, ConfigError> {
        if self.path.is_none() {
            return Err(no_data_dir());
        }
        let (current, dynamic_now) = {
            let st = self.lock();
            if let Some(e) = &st.load_error {
                return Err(ConfigError::Conflict(e.clone()));
            }
            (st.managed.clone(), st.dynamic.clone())
        };
        let (next, summary, warnings) = self.plan(ctx, &current, &dynamic_now, &change)?;
        let id = an_core::new_id("prop");
        let token = format!("{}{}", an_core::new_id("tok"), an_core::new_id(""));
        let mut st = self.lock();
        let ttl = st.ttl;
        let now = Instant::now();
        st.proposals.retain(|_, p| p.expires > now);
        st.proposals.insert(
            id.clone(),
            Pending {
                change: change.clone(),
                token: token.clone(),
                expires: now + ttl,
            },
        );
        tracing::info!(event = "agent.config_proposed", proposal_id = %id, kind = change.kind(), summary = %summary);
        Ok(Proposal {
            id,
            kind: change.kind().into(),
            summary,
            warnings,
            before: current.to_toml(),
            after: next.to_toml(),
            token,
            expires_in_secs: ttl.as_secs(),
        })
    }

    /// 确认：核对凭证，在当前状态上重新计算并写入，然后更新注册表
    pub async fn apply(
        &self,
        ctx: &Ctx<'_>,
        id: &str,
        token: &str,
    ) -> Result<Applied, ConfigError> {
        let _serial = self.apply_lock.lock().await;
        let (change, current, dynamic_now) = {
            let mut st = self.lock();
            let Some(p) = st.proposals.get(id) else {
                return Err(ConfigError::NotFound(format!("提议 {id} 不存在")));
            };
            if p.token != token {
                tracing::warn!(event = "agent.config_rejected", proposal_id = %id, reason = "bad_token");
                return Err(ConfigError::BadToken);
            }
            if p.expires <= Instant::now() {
                st.proposals.remove(id);
                tracing::warn!(event = "agent.config_rejected", proposal_id = %id, reason = "expired");
                return Err(ConfigError::Expired);
            }
            if let Some(e) = &st.load_error {
                return Err(ConfigError::Conflict(e.clone()));
            }
            let change = st.proposals.remove(id).map(|p| p.change).expect("刚检查过");
            (change, st.managed.clone(), st.dynamic.clone())
        };
        // 在最新状态上重新计算（两次提议之间可能有别的修改生效）
        let (next, summary, _) = self.plan(ctx, &current, &dynamic_now, &change)?;
        let path = self.path.clone().ok_or_else(no_data_dir)?;
        if matches!(change, ChangeRequest::Revert) {
            write_atomic(&path, &next.to_toml())?;
            self.pop_history();
        } else {
            self.push_history(&current)?;
            write_atomic(&path, &next.to_toml())?;
        }
        let probes = self.reconcile(ctx, next).await;
        let event = if matches!(change, ChangeRequest::Revert) {
            "agent.config_reverted"
        } else {
            "agent.config_applied"
        };
        tracing::info!(event, proposal_id = %id, kind = change.kind(), summary = %summary);
        Ok(Applied {
            kind: change.kind().into(),
            summary,
            probes,
        })
    }

    /// 按新的托管内容更新注册表：增删 agent、更新执行组；新接入的 agent 立即做一次握手检测
    async fn reconcile(&self, ctx: &Ctx<'_>, next: ManagedConfig) -> Vec<ProbeOutcome> {
        let desired = dynamic_desired(ctx.config, &next);
        let dynamic_now = self.lock().dynamic.clone();
        // 1. 移除：不再需要的，或配置变了的
        for (id, old) in &dynamic_now {
            let keep = desired.iter().any(|d| &d.id == id && same_config(d, old));
            if !keep {
                ctx.registry.unregister_local(id);
                ctx.probers.remove(id);
            }
        }
        // 2. 新增
        let mut dynamic = BTreeMap::new();
        let mut added = vec![];
        for cfg in desired {
            let unchanged = dynamic_now
                .get(&cfg.id)
                .is_some_and(|old| same_config(old, &cfg));
            if unchanged {
                dynamic.insert(cfg.id.clone(), cfg);
                continue;
            }
            let agent = Arc::new(AcpAgent::new(
                resolve_env(&cfg),
                default_workdir(ctx.config, cfg.role),
            ));
            if ctx.registry.register(agent.clone()) {
                ctx.probers.add(Prober::Acp(agent.clone()));
                added.push(agent);
                dynamic.insert(cfg.id.clone(), cfg);
            }
        }
        // 3. 执行组
        ctx.registry.set_assign(merged_assign(
            &ctx.config.agents.assign_map().unwrap_or_default(),
            &next,
        ));
        {
            let mut st = self.lock();
            st.managed = next;
            st.dynamic = dynamic;
        }
        // 4. 新接入的立即检测一次（结果同步写入连接状态）
        let mut probes = vec![];
        for a in added {
            let out = a
                .probe_detailed(Duration::from_secs(PROBE_TIMEOUT_SECS))
                .await;
            ctx.registry
                .links
                .record_probe(&a.card().id, an_agents::acp::probe_result(&out));
            probes.push(out);
        }
        probes
    }

    /// 计算修改后的托管内容，并生成给用户看的说明和警告
    fn plan(
        &self,
        ctx: &Ctx<'_>,
        current: &ManagedConfig,
        dynamic_now: &BTreeMap<String, AcpConfig>,
        change: &ChangeRequest,
    ) -> Result<(ManagedConfig, String, Vec<String>), ConfigError> {
        let config = ctx.config;
        let in_toml = |id: &str| config.acp.agents.iter().any(|a| a.id == id);
        let toml_refuse = |what: &str| {
            ConfigError::Conflict(format!(
                "{what}写在你手动维护的 config.toml 里，这里不能修改；如需修改，请直接编辑 config.toml 后重启"
            ))
        };
        let mut next = current.clone();
        let mut warnings = vec![];
        let summary;
        match change {
            ChangeRequest::AddPreset { id } => {
                let Some(preset) = presets().into_iter().find(|p| &p.id == id) else {
                    return Err(ConfigError::Invalid(format!("目录里没有 {id}")));
                };
                if in_toml(id) {
                    return Err(toml_refuse(&format!("agent {id} ")));
                }
                if dynamic_now.contains_key(id) && !current.acp.disabled.contains(id) {
                    return Err(ConfigError::Conflict(format!(
                        "{id} 已经接入了，不需要重复接入"
                    )));
                }
                next.acp.disabled.retain(|d| d != id);
                let installed = an_agents::util::resolve_command(&preset.command).is_some();
                let auto = config.acp.auto_detect && installed;
                if !auto && !next.acp.agents.iter().any(|a| &a.id == id) {
                    next.acp.agents.push(preset.clone());
                }
                if !installed {
                    warnings.push(format!(
                        "本机还找不到 {} 的启动命令 `{}`，接入后会显示为不可用；请先按目录里的安装步骤安装",
                        preset.name.clone().unwrap_or(preset.id.clone()),
                        preset.command
                    ));
                }
                summary = format!(
                    "接入「{}」，以后可以把「{}」类的任务交给它",
                    preset.name.unwrap_or(preset.id.clone()),
                    role_label(preset.role)
                );
            }
            ChangeRequest::AddCustom { agent } => {
                let id = agent.id.trim();
                if id.is_empty() || id.contains('@') || id.contains(char::is_whitespace) {
                    return Err(ConfigError::Invalid(
                        "id 不能为空，也不能包含 @ 或空格".into(),
                    ));
                }
                if in_toml(id) {
                    return Err(toml_refuse(&format!("agent {id} ")));
                }
                let builtin = ctx
                    .registry
                    .cards()
                    .into_iter()
                    .any(|c| c.id == id && c.kind != "acp");
                if builtin {
                    return Err(ConfigError::Conflict(format!(
                        "{id} 是内置或其他方式接入的 agent，不能用同一个 id"
                    )));
                }
                if current.acp.agents.iter().any(|a| a.id == id) {
                    return Err(ConfigError::Conflict(format!(
                        "托管配置里已经有 {id}，请先移除再接入"
                    )));
                }
                if let Some((k, _)) = agent.env.iter().find(|(_, v)| !is_env_reference(v)) {
                    return Err(ConfigError::Invalid(format!(
                        "环境变量 {k} 只能写成 ${{变量名}} 的引用（例如 ${{MY_API_KEY}}），不要把密钥写进来；请先在启动 AgentNexus 的终端里 export 这个变量"
                    )));
                }
                if agent.install_hint.is_some() {
                    return Err(ConfigError::Invalid(
                        "自定义 agent 不能设置 install_hint".into(),
                    ));
                }
                if an_agents::util::resolve_command(&agent.command).is_none() {
                    warnings.push(format!(
                        "本机找不到命令 `{}`，接入后会显示为不可用",
                        agent.command
                    ));
                }
                warnings.push(format!(
                    "确认后，系统会在需要时运行命令：{} {}",
                    agent.command,
                    agent.args.join(" ")
                ));
                let mut a = agent.clone();
                a.id = id.to_string();
                next.acp.agents.push(a);
                next.acp.disabled.retain(|d| d != id);
                summary = format!(
                    "接入自定义 agent「{}」，负责「{}」类任务",
                    agent.name.clone().unwrap_or(id.to_string()),
                    role_label(agent.role)
                );
            }
            ChangeRequest::SetDefault { role, ids } => {
                if config.agents.assign.contains_key(role.as_str()) {
                    return Err(toml_refuse(&format!("「{}」的执行者", role_label(*role))));
                }
                let desired = dynamic_desired(config, &next);
                let cards = ctx.preview_cards(dynamic_now, &desired);
                check_group(&cards, *role, ids).map_err(ConfigError::Invalid)?;
                next.agents.assign.insert(role.as_str().into(), ids.clone());
                summary = format!(
                    "以后「{}」类的任务交给 {}{}",
                    role_label(*role),
                    ids.join("、"),
                    if ids.len() > 1 {
                        "（第一个是主力，其余是备份）"
                    } else {
                        ""
                    }
                );
            }
            ChangeRequest::Disable { id } => {
                if in_toml(id) {
                    return Err(toml_refuse(&format!("agent {id} ")));
                }
                let known = dynamic_now.contains_key(id)
                    || current.acp.agents.iter().any(|a| &a.id == id)
                    || presets().iter().any(|p| &p.id == id);
                if !known {
                    let other = ctx.registry.cards().into_iter().any(|c| &c.id == id);
                    return Err(if other {
                        ConfigError::Conflict(format!(
                            "{id} 是内置或其他方式接入的 agent，这里不能停用，请改 config.toml"
                        ))
                    } else {
                        ConfigError::NotFound(format!("没有名为 {id} 的 agent"))
                    });
                }
                if current.acp.disabled.contains(id) {
                    return Err(ConfigError::Conflict(format!("{id} 已经停用了")));
                }
                next.acp.disabled.push(id.clone());
                for ids in next.agents.assign.values_mut() {
                    ids.retain(|x| x != id);
                }
                next.agents.assign.retain(|_, ids| !ids.is_empty());
                // 停用后这个角色是否还有可用的 agent
                let role = ctx
                    .registry
                    .cards()
                    .into_iter()
                    .find(|c| &c.id == id)
                    .map(|c| c.role);
                if let Some(role) = role {
                    let desired = dynamic_desired(config, &next);
                    let cards = ctx.preview_cards(dynamic_now, &desired);
                    if !cards.iter().any(|c| c.role == role && c.enabled) {
                        warnings.push(format!(
                            "停用后，「{}」类的任务将没有执行者：排队中的这类任务会失败，之后这类请求由我在对话里直接回复（例如给出建议），不会自动改交给别的助手",
                            role_label(role)
                        ));
                    }
                }
                summary = format!("停用 {id}：之后不再把任务交给它，正在做的任务会照常做完");
            }
            ChangeRequest::Enable { id } => {
                if !current.acp.disabled.contains(id) {
                    return Err(ConfigError::Conflict(format!("{id} 没有被停用")));
                }
                next.acp.disabled.retain(|d| d != id);
                summary = format!("恢复启用 {id}");
            }
            ChangeRequest::Remove { id } => {
                if in_toml(id) {
                    return Err(toml_refuse(&format!("agent {id} ")));
                }
                if !current.acp.agents.iter().any(|a| &a.id == id) {
                    return Err(ConfigError::NotFound(format!(
                        "托管配置里没有 {id}（自动发现的预设请用「停用」）"
                    )));
                }
                next.acp.agents.retain(|a| &a.id != id);
                for ids in next.agents.assign.values_mut() {
                    ids.retain(|x| x != id);
                }
                next.agents.assign.retain(|_, ids| !ids.is_empty());
                summary = format!("移除自定义 agent {id}");
            }
            ChangeRequest::Revert => {
                let Some(prev) = self.history_top() else {
                    return Err(ConfigError::Conflict("没有可以撤销的修改".into()));
                };
                next = prev;
                summary = "撤销最近一次修改，恢复到上一个版本".into();
            }
        }
        Ok((next, summary, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_reference_format() {
        assert!(is_env_reference("${MY_KEY}"));
        assert!(is_env_reference("${_A1}"));
        assert!(!is_env_reference("sk-123"));
        assert!(!is_env_reference("${1A}"));
        assert!(!is_env_reference("${A-B}"));
        assert!(!is_env_reference("prefix${A}"));
    }

    #[test]
    fn merged_assign_prefers_config_toml() {
        let mut managed = ManagedConfig::default();
        managed
            .agents
            .assign
            .insert("coding".into(), vec!["m".into()]);
        managed
            .agents
            .assign
            .insert("general".into(), vec!["g".into()]);
        let cfg: BTreeMap<String, Vec<String>> =
            [("coding".to_string(), vec!["t".to_string()])].into();
        let m = merged_assign(&cfg, &managed);
        assert_eq!(m["coding"], vec!["t"]);
        assert_eq!(m["general"], vec!["g"]);
    }

    #[test]
    fn managed_round_trips_through_toml() {
        let mut m = ManagedConfig::default();
        m.acp.disabled.push("zcode".into());
        m.agents
            .assign
            .insert("coding".into(), vec!["cursor".into()]);
        let back: ManagedConfig = toml::from_str(&m.to_toml()).unwrap();
        assert_eq!(back.acp.disabled, vec!["zcode"]);
        assert_eq!(back.agents.assign["coding"], vec!["cursor"]);
    }
}

// ───────────────────────── Runtime 入口 ─────────────────────────

impl crate::Runtime {
    fn agent_ctx(&self) -> Ctx<'_> {
        Ctx {
            config: &self.config,
            registry: &self.registry,
            probers: &self.probers,
        }
    }

    /// 提议一次 agent 配置修改（只校验、不写入）
    pub fn propose_agent_change(&self, change: ChangeRequest) -> Result<Proposal, ConfigError> {
        self.agent_config.propose(&self.agent_ctx(), change)
    }

    /// 确认一次提议（必须带提议时返回的凭证）
    pub async fn apply_agent_proposal(
        &self,
        id: &str,
        token: &str,
    ) -> Result<Applied, ConfigError> {
        self.agent_config.apply(&self.agent_ctx(), id, token).await
    }
}
