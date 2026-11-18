//! 对话里管理助手（AG.4）：把"管理助手"的判断变成卡片。
//!
//! - 模型只做选择题：动作 manage_agents + 操作 / 助手 / 角色（都是候选枚举）；
//! - 之后全部由这里的代码完成：生成提议、选择卡、引导卡、发现卡；不调用模型；
//! - 卡片存在消息的 card_json；消息正文是大白话摘要，送给模型的历史只用正文，卡片数据（含凭证）不进模型上下文；
//! - 用户在卡片上确认后，结果卡写回原来的会话，原确认卡标记为已确认 / 已过期。
//!
//! HTTP 接口和对话流程共用 [`AgentAdmin`]。

use std::sync::Arc;

use an_agents::acp::presets;
use an_core::{
    AgentOp, CatalogItem, CatalogStatus, ChatCard, ChoiceOption, MessageView, ProbeKind,
    ProbeOutcome, Role,
};

use crate::agent_catalog::catalog_items;
use crate::agent_config::{AgentConfigManager, Applied, ChangeRequest, ConfigError, Ctx};
use crate::bus::Bus;
use crate::config::AppConfig;
use crate::registry::{role_label, AgentRegistry};
use crate::store::Store;
use crate::ProberSet;

/// 管理助手需要的运行时部件（HTTP 接口和对话流程共用）
pub struct AgentAdmin {
    pub config: AppConfig,
    pub registry: Arc<AgentRegistry>,
    pub probers: Arc<ProberSet>,
    pub manager: Arc<AgentConfigManager>,
    pub store: Arc<Store>,
    pub bus: Arc<Bus>,
}

/// 检测结果 → 大白话（与前端 probeText 一致）
pub fn probe_plain(p: &ProbeOutcome) -> String {
    let next = p
        .next_step
        .as_ref()
        .map(|n| format!("下一步：{n}"))
        .unwrap_or_default();
    match p.kind {
        ProbeKind::Ok => "已连通，可以把任务交给它".into(),
        ProbeKind::NotInstalled => format!("还没有安装（本机找不到它的启动命令）。{next}"),
        ProbeKind::NeedsLogin => format!("需要先登录。{next}"),
        ProbeKind::MissingDependency => format!("缺少运行需要的程序。{next}"),
        ProbeKind::Timeout => format!("没有在规定时间内响应。{next}"),
        ProbeKind::HandshakeFailed => format!("连接失败。{next}"),
    }
}

impl AgentAdmin {
    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            config: &self.config,
            registry: &self.registry,
            probers: &self.probers,
        }
    }

    /// 目录（预设 + 本机状态）
    pub fn catalog(&self) -> Vec<CatalogItem> {
        catalog_items(&self.config, &self.registry, &self.manager)
    }

    /// 模型可以选的助手 id：全部预设 + 已注册的 + 托管的（含已停用的）。按字母排序，保证每轮相同
    pub fn candidate_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = presets().into_iter().map(|p| p.id).collect();
        ids.extend(self.registry.cards().into_iter().map(|c| c.id));
        ids.extend(self.manager.managed().acp.agents.into_iter().map(|a| a.id));
        ids.sort();
        ids.dedup();
        ids
    }

    /// 给前台判断的提示词：名称 → id 对照（模型从中选，不能自己编）
    pub fn prompt_section(&self) -> String {
        let catalog = self.catalog();
        let cards = self.registry.cards();
        let mut lines = vec![];
        for id in self.candidate_ids() {
            let (name, extra) = if let Some(c) = catalog.iter().find(|c| c.id == id) {
                (c.name.clone(), format!("命令 {}，{}", c.command, c.purpose))
            } else if let Some(c) = cards.iter().find(|c| c.id == id) {
                (c.name.clone(), c.description.clone())
            } else {
                (id.clone(), String::new())
            };
            lines.push(format!("- {id}：{name}（{extra}）"));
        }
        format!(
            "\n\n## 管理助手（在第 1 条之后、第 2 条之前判断）\n\
             用户在说**助手本身**时，action=manage_agents：要接入 / 接进来（connect）、以后某类任务交给谁（set_default）、\
             停用 / 停掉（disable）、恢复启用（enable）、看看有哪些助手（list）、撤销刚才对助手的修改（undo）。\n\
             - agent_op 填上面括号里的英文；agent_id 只能从下面的列表里选，用户没说是哪个就填空字符串；\
             agent_role 是这个助手负责的事：coding（写代码）/ general（通用任务）/ reviewer（审查），不确定填空字符串。\n\
             - 让某个助手**干活**（例如\"用 Cursor 帮我改这个函数\"）不是这一条，按第 2 条交给后台；\
             比较、询问助手（例如\"Cursor 和 Claude Code 哪个好\"\"Cursor 怎么安装\"）不是这一条，直接回答。\n\
             - 不适用时 agent_op、agent_id、agent_role 都填空字符串。\n\
             可选的助手（id：名称）：\n{}",
            lines.join("\n")
        )
    }

    /// 生成确认卡：提议（只校验、不写入）+ 大白话正文。失败时返回大白话原因
    pub fn confirm_card(&self, change: ChangeRequest) -> Result<(String, ChatCard), ConfigError> {
        let command = match &change {
            ChangeRequest::AddPreset { id } => presets()
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| [vec![p.command], p.args].concat().join(" ")),
            ChangeRequest::AddCustom { agent } => Some(
                [vec![agent.command.clone()], agent.args.clone()]
                    .concat()
                    .join(" "),
            ),
            _ => None,
        };
        let p = self.manager.propose(&self.ctx(), change)?;
        let minutes = (p.expires_in_secs / 60).max(1);
        let content = format!(
            "{}。请在下面的确认卡上点「确认」后生效（{minutes} 分钟内有效）。",
            p.summary
        );
        let card = ChatCard::Confirm {
            proposal_id: p.id,
            kind: p.kind,
            summary: p.summary,
            warnings: p.warnings,
            before: p.before,
            after: p.after,
            token: p.token,
            expires_in_secs: p.expires_in_secs,
            command,
            state: "pending".into(),
        };
        Ok((content, card))
    }

    /// 在会话里追加一条带卡片的助手消息，并通知界面刷新
    pub fn post_card(
        &self,
        conversation_id: &str,
        content: &str,
        card: &ChatCard,
    ) -> anyhow::Result<MessageView> {
        let message =
            self.store
                .add_message_with_card(conversation_id, "assistant", content, card)?;
        if let ChatCard::Confirm { proposal_id, .. } = card {
            self.manager
                .set_origin(proposal_id, conversation_id, &message.id);
        }
        self.bus.assistant_message(conversation_id, None, content);
        Ok(MessageView {
            message,
            card: Some(card.clone()),
        })
    }

    /// 会话里的选择卡被点选后：生成确认卡并追加到会话（HTTP 接口 /api/conversations/{id}/agent-proposals）
    pub fn propose_in_conversation(
        &self,
        conversation_id: &str,
        change: ChangeRequest,
    ) -> Result<MessageView, ConfigError> {
        let (content, card) = self.confirm_card(change)?;
        self.post_card(conversation_id, &content, &card)
            .map_err(|e| ConfigError::Internal(e.to_string()))
    }

    /// 确认提议。来自对话的提议：成功后追加结果卡、原确认卡标记 applied；过期时标记 expired
    pub async fn apply(&self, id: &str, token: &str) -> Result<Applied, ConfigError> {
        let origin = self.manager.origin_of(id);
        let result = self.manager.apply(&self.ctx(), id, token).await;
        if let Some((conv, msg_id)) = origin {
            let mark = |state: &str| {
                if let Ok(Some(ChatCard::Confirm {
                    proposal_id,
                    kind,
                    summary,
                    warnings,
                    before,
                    after,
                    token,
                    expires_in_secs,
                    command,
                    ..
                })) = self.store.message_card(&msg_id)
                {
                    let card = ChatCard::Confirm {
                        proposal_id,
                        kind,
                        summary,
                        warnings,
                        before,
                        after,
                        token,
                        expires_in_secs,
                        command,
                        state: state.into(),
                    };
                    if let Err(e) = self.store.update_message_card(&msg_id, &card) {
                        tracing::error!(error = %e, message_id = %msg_id, "更新确认卡状态失败");
                    }
                }
            };
            match &result {
                Ok(applied) => {
                    mark("applied");
                    let mut content = format!("已完成：{}", applied.summary);
                    for p in &applied.probes {
                        content.push_str(&format!("\n检测「{}」：{}", p.id, probe_plain(p)));
                    }
                    let card = ChatCard::Result {
                        ok: true,
                        summary: applied.summary.clone(),
                        probes: applied.probes.clone(),
                        error: None,
                    };
                    if let Err(e) = self.post_card(&conv, &content, &card) {
                        tracing::error!(error = %e, "写入结果卡失败");
                    }
                }
                Err(ConfigError::Expired) => {
                    mark("expired");
                    self.bus.assistant_message(&conv, None, "确认卡已过期");
                }
                Err(_) => {}
            }
        }
        result
    }

    /// 前台判断为 manage_agents 后的向导：返回 (大白话正文, 卡片)
    pub fn wizard(
        &self,
        op: AgentOp,
        agent_id: &str,
        role: Option<Role>,
    ) -> (String, Option<ChatCard>) {
        let catalog = self.catalog();
        let cards = self.registry.cards();
        let managed = self.manager.managed();
        let role_ok = |r: Role| role.is_none_or(|want| want == r);
        let name_of = |id: &str| {
            catalog
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .or_else(|| cards.iter().find(|c| c.id == id).map(|c| c.name.clone()))
                .unwrap_or_else(|| id.to_string())
        };
        let label = role.map(role_label).unwrap_or("这类任务");
        let choice = |options: Vec<ChoiceOption>, what: &str| -> (String, Option<ChatCard>) {
            if options.is_empty() {
                return (
                    format!("现在没有可以{what}的助手。可以在右侧「助手」页签里查看全部助手。"),
                    None,
                );
            }
            (
                format!("你想{what}哪一个？在下面的卡片上点选就行。"),
                Some(ChatCard::Choice { op, role, options }),
            )
        };
        let change = match op {
            AgentOp::List => {
                let connected = catalog
                    .iter()
                    .filter(|c| c.status == CatalogStatus::Registered && !c.disabled)
                    .count();
                let installable = catalog
                    .iter()
                    .filter(|c| c.status == CatalogStatus::Installed && !c.disabled)
                    .count();
                let missing = catalog
                    .iter()
                    .filter(|c| c.status == CatalogStatus::NotInstalled)
                    .count();
                return (
                    format!("这是本机可以用的助手：已接入 {connected} 个，已安装可以接入 {installable} 个，还没安装 {missing} 个。可以直接点卡片上的按钮。"),
                    Some(ChatCard::Discovery { items: catalog }),
                );
            }
            AgentOp::Undo => ChangeRequest::Revert,
            AgentOp::Connect => {
                if agent_id.is_empty() {
                    let options = catalog
                        .iter()
                        .filter(|c| {
                            c.status != CatalogStatus::Registered
                                && role_ok(c.role)
                                && c.source != "config_toml"
                        })
                        .map(|c| ChoiceOption {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            purpose: c.purpose.clone(),
                            role: c.role,
                            installed: c.status != CatalogStatus::NotInstalled,
                        })
                        .collect();
                    return choice(options, &format!("接入做「{label}」"));
                }
                match catalog.iter().find(|c| c.id == agent_id) {
                    Some(item) if item.status == CatalogStatus::NotInstalled => {
                        return (
                            format!("「{}」还没有安装。按下面的步骤装好后，点「我装好了，重新检测」，再跟我说一次「把它接进来」就行。", item.name),
                            Some(ChatCard::Guide { item: item.clone() }),
                        );
                    }
                    Some(_) => ChangeRequest::AddPreset { id: agent_id.into() },
                    None if managed.acp.disabled.iter().any(|d| d == agent_id) => {
                        ChangeRequest::Enable { id: agent_id.into() }
                    }
                    None => {
                        return (
                            format!("「{}」不在可以直接接入的目录里。自定义的助手可以在右侧「助手」页签点「添加自定义助手」。", name_of(agent_id)),
                            None,
                        )
                    }
                }
            }
            AgentOp::SetDefault => {
                if agent_id.is_empty() {
                    let options = cards
                        .iter()
                        .filter(|c| c.enabled && c.role != Role::Reviewer && role_ok(c.role))
                        .map(|c| ChoiceOption {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            purpose: c.description.clone(),
                            role: c.role,
                            installed: true,
                        })
                        .collect();
                    return choice(options, &format!("设为「{label}」默认"));
                }
                let Some(card) = cards.iter().find(|c| c.id == agent_id) else {
                    return (
                        format!("「{}」还没有接入，先接入它才能设为默认。要接入的话跟我说「把它接进来」。", name_of(agent_id)),
                        None,
                    );
                };
                ChangeRequest::SetDefault {
                    role: card.role,
                    ids: vec![agent_id.into()],
                }
            }
            AgentOp::Disable => {
                if agent_id.is_empty() {
                    let dynamic: Vec<&str> = catalog
                        .iter()
                        .filter(|c| {
                            c.status == CatalogStatus::Registered && c.source != "config_toml"
                        })
                        .map(|c| c.id.as_str())
                        .chain(managed.acp.agents.iter().map(|a| a.id.as_str()))
                        .collect();
                    let options = cards
                        .iter()
                        .filter(|c| dynamic.contains(&c.id.as_str()) && role_ok(c.role))
                        .map(|c| ChoiceOption {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            purpose: c.description.clone(),
                            role: c.role,
                            installed: true,
                        })
                        .collect();
                    return choice(options, "停用");
                }
                ChangeRequest::Disable {
                    id: agent_id.into(),
                }
            }
            AgentOp::Enable => {
                if agent_id.is_empty() {
                    let options = managed
                        .acp
                        .disabled
                        .iter()
                        .filter_map(|id| {
                            let item = catalog.iter().find(|c| &c.id == id);
                            let custom = managed.acp.agents.iter().find(|a| &a.id == id);
                            let role = item.map(|i| i.role).or(custom.map(|a| a.role))?;
                            role_ok(role).then(|| ChoiceOption {
                                id: id.clone(),
                                name: name_of(id),
                                purpose: item.map(|i| i.purpose.clone()).unwrap_or_default(),
                                role,
                                installed: true,
                            })
                        })
                        .collect();
                    return choice(options, "恢复启用");
                }
                ChangeRequest::Enable {
                    id: agent_id.into(),
                }
            }
        };
        match self.confirm_card(change) {
            Ok((content, card)) => (content, Some(card)),
            Err(e) => (e.to_string(), None),
        }
    }
}
