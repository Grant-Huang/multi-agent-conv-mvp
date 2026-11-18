//! Agent 目录与握手检测（AG.1，只读）。
//!
//! - [`Runtime::agent_catalog`]：列出全部预设及其在本机的状态（未安装 / 已安装未注册 / 已注册）；
//! - [`Runtime::probe_agent`]：对目录内或已注册的 ACP agent 做一次握手检测并分类。
//!
//! 只读：不改注册表、不写配置。检测只接受 agent id，启动的命令来自预设或已有配置，
//! 不接受调用方传入的命令。
//!
//! 超时、取消与清理：每次检测最多 `PROBE_TIMEOUT_SECS` 秒；调用方（HTTP 请求）中途取消时，
//! 检测的 future 被丢弃，连接释放时 SDK 结束 agent 的整个进程组。
//! 并发：同一个 agent 同时只做一次检测（后到的等前一次结束），全局最多同时 `MAX_CONCURRENT_PROBES` 个。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_agents::acp::{probe_result, PROBE_TIMEOUT_SECS};
use an_agents::catalog::catalog;
use an_agents::util::resolve_command;
use an_agents::AcpAgent;
use an_core::{CatalogItem, CatalogStatus, ProbeKind, ProbeOutcome, Role};
use tokio::sync::Semaphore;

use crate::{Prober, Runtime};

/// 全局同时进行的握手检测上限（每次检测会启动一个子进程）
pub const MAX_CONCURRENT_PROBES: usize = 2;

/// 握手检测的并发闸门
pub struct ProbeGate {
    global: Semaphore,
    /// 每个 agent 一把锁：同一个 agent 不并发检测
    per_agent: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Default for ProbeGate {
    fn default() -> Self {
        Self {
            global: Semaphore::new(MAX_CONCURRENT_PROBES),
            per_agent: Mutex::default(),
        }
    }
}

impl ProbeGate {
    fn agent_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.per_agent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id.to_string())
            .or_default()
            .clone()
    }
}

/// 要检测的对象
struct Target {
    agent: Arc<AcpAgent>,
    /// 是否已注册（已注册的检测结果同步写入连接状态）
    registered: bool,
    /// 目录里的登录说明（需要登录时作为下一步）
    login: Option<&'static str>,
}

impl Runtime {
    /// 目录：全部预设 + 本机状态。连接状态（link_status / link_reason）由调用方按健康接口的规则填写。
    pub fn agent_catalog(&self) -> Vec<CatalogItem> {
        catalog_items(&self.config, &self.registry, &self.agent_config)
    }

    /// 排查（AG.5）：握手检测 + 本机体检 + 已知问题规则（第 1、2 层），再附上诊断包（第 3 层）。
    /// 系统不往外发；配置了 DLP 时诊断包经它检查（渠道 clipboard），用完即撤销。
    /// 只能排查目录内或已注册的 ACP agent；其他 id 返回 None（HTTP 404）。
    pub async fn diagnose_agent(&self, id: &str) -> Option<an_core::DiagnoseReport> {
        let (mut report, local_text) = self.diagnose_core(id).await?;
        report.package = self.diagnose_package(local_text).await;
        Some(report)
    }

    /// 第 1、2 层，外加本机处理过的诊断包原文（第 3、4 层共用）。不调用 DLP
    pub(crate) async fn diagnose_core(
        &self,
        id: &str,
    ) -> Option<(an_core::DiagnoseReport, String)> {
        use an_agents::diagnose::{
            check_items, checkup, package_text, rules, CheckupInput, PackageInput, RuleContext,
        };
        let probe = self.probe_agent(id).await?;
        let entry = catalog().into_iter().find(|e| e.id() == id);
        // 实际使用的命令：已注册的用注册时的配置，否则用目录预设
        let registered_cfg = self.probers.snapshot().into_iter().find_map(|p| match p {
            Prober::Acp(a) if an_core::AgentAdapter::card(a.as_ref()).id == id => {
                Some(a.config().clone())
            }
            _ => None,
        });
        let (command, args, name) = match (&registered_cfg, &entry) {
            (Some(c), _) => (
                c.command.clone(),
                c.args.clone(),
                c.name.clone().unwrap_or_else(|| c.id.clone()),
            ),
            (None, Some(e)) => (e.preset.command.clone(), e.preset.args.clone(), e.name()),
            (None, None) => return None,
        };
        // 诊断包里环境变量只列名字，不带值
        let env_names: Vec<String> = registered_cfg
            .as_ref()
            .map(|c| c.env.keys().cloned().collect())
            .unwrap_or_default();
        // 只有命令与目录预设完全相同时才算预设（才做登录 shell 检查、node 和凭证检查）
        let preset = entry.as_ref().filter(|e| e.preset.command == command);
        let input = CheckupInput {
            command: command.clone(),
            is_preset: preset.is_some(),
            node_min_major: preset.and_then(|e| e.meta.node_min_major),
            credential_files: preset
                .map(|e| {
                    e.meta
                        .credential_files
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let c = checkup(&input).await;
        let ctx = RuleContext {
            name: name.clone(),
            command: command.clone(),
            args: args.clone(),
            install: entry
                .as_ref()
                .map(|e| e.meta.install.clone())
                .unwrap_or_default(),
            login: entry.as_ref().and_then(|e| e.meta.login.map(String::from)),
        };
        let findings = rules(&probe, &c, &ctx);
        let unmatched = probe.kind != ProbeKind::Ok && findings.is_empty();
        if unmatched {
            // 记录特征以便补规则：只记分类和错误输出的长度，不记内容
            tracing::info!(
                event = "agent.diagnose_unmatched",
                agent_id = %id,
                kind = ?probe.kind,
                detail_len = probe.detail.as_deref().map(str::len).unwrap_or(0),
                "排查没有命中任何规则"
            );
        }
        tracing::info!(event = "agent.diagnose", agent_id = %id, kind = ?probe.kind, findings = findings.len());
        let checks = check_items(&c);
        let local_text = package_text(&PackageInput {
            id,
            name: &name,
            command: &command,
            args: &args,
            env_names,
            probe: &probe,
            checks: &checks,
            findings: &findings,
        });
        Some((
            an_core::DiagnoseReport {
                id: id.to_string(),
                name,
                checks,
                probe,
                findings,
                unmatched,
                package: an_core::DiagnosePackage::default(),
            },
            local_text,
        ))
    }

    /// 握手检测。只能检测已注册的 ACP agent 或目录内的预设；其他 id（未知、内置、CLI、节点）返回 None。
    pub async fn probe_agent(&self, id: &str) -> Option<ProbeOutcome> {
        let target = self.probe_target(id)?;
        let lock = self.probe_gate.agent_lock(id);
        let _same_agent = lock.lock().await;
        // 未安装时不启动进程，也不占全局名额
        let _slot = if target.agent.is_installed() {
            Some(self.probe_gate.global.acquire().await.ok()?)
        } else {
            None
        };
        let mut out = target
            .agent
            .probe_detailed(Duration::from_secs(PROBE_TIMEOUT_SECS))
            .await;
        if out.kind == ProbeKind::NeedsLogin {
            if let Some(login) = target.login {
                out.next_step = Some(format!("{login}，然后重新检测"));
            }
        }
        if target.registered {
            self.registry.links.record_probe(id, probe_result(&out));
        }
        tracing::info!(
            event = "agent.probe",
            agent_id = %id,
            kind = ?out.kind,
            registered = target.registered,
            "握手检测"
        );
        Some(out)
    }

    fn probe_target(&self, id: &str) -> Option<Target> {
        let login = catalog()
            .into_iter()
            .find(|e| e.id() == id)
            .and_then(|e| e.meta.login);
        // 已注册的 ACP agent：用注册时的同一个实例（命令、参数、工作目录都来自配置）
        for p in self.probers.snapshot().iter() {
            if let Prober::Acp(a) = p {
                if an_core::AgentAdapter::card(a.as_ref()).id == id {
                    return Some(Target {
                        agent: a.clone(),
                        registered: true,
                        login,
                    });
                }
            }
        }
        // 同 id 已被其他类型的 agent 占用（内置、CLI、节点）：不检测
        if self.registry.cards().iter().any(|c| c.id == id) {
            return None;
        }
        // 目录内但未注册的预设：临时构造一个实例，只用于这次检测
        let entry = catalog().into_iter().find(|e| e.id() == id)?;
        let workdir = match entry.role() {
            Role::Coding => self.config.agents.coding.cfg.workdir.clone(),
            _ => self.config.agents.general.workspace_dir.clone(),
        };
        Some(Target {
            agent: Arc::new(AcpAgent::new(entry.preset, workdir)),
            registered: false,
            login,
        })
    }
}

/// 目录：全部预设 + 本机状态（Runtime 和对话里的向导共用）
pub fn catalog_items(
    config: &crate::config::AppConfig,
    registry: &crate::registry::AgentRegistry,
    manager: &crate::agent_config::AgentConfigManager,
) -> Vec<CatalogItem> {
    let cards = registry.cards();
    let managed = manager.managed();
    catalog()
        .into_iter()
        .map(|e| {
            let registered = cards.iter().any(|c| c.id == e.id());
            let status = if registered {
                CatalogStatus::Registered
            } else if resolve_command(&e.preset.command).is_some() {
                CatalogStatus::Installed
            } else {
                CatalogStatus::NotInstalled
            };
            let source = if config.acp.agents.iter().any(|a| a.id == e.id()) {
                "config_toml"
            } else if managed.acp.agents.iter().any(|a| a.id == e.id()) {
                "managed"
            } else if registered {
                "auto"
            } else {
                "preset"
            };
            CatalogItem {
                id: e.id().to_string(),
                source: source.into(),
                disabled: managed.acp.disabled.iter().any(|d| d == e.id()),
                name: e.name(),
                purpose: e.meta.purpose.into(),
                role: e.role(),
                command: e.preset.command.clone(),
                args: e.preset.args.clone(),
                install: e.meta.install.clone(),
                login: e.meta.login.map(String::from),
                requirements: e.meta.requirements.iter().map(|s| s.to_string()).collect(),
                docs_url: e.meta.docs_url.map(String::from),
                status,
                link_status: None,
                link_reason: None,
            }
        })
        .collect()
}
