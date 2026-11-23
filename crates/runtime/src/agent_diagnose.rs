//! 排查第 3、4 层（AG.5）：诊断包；请其他助手帮忙查（每次预览、逐次同意，经 DLP）。
//!
//! 设计（docs/DECISIONS.md 第 12～14、17、41 条；DLP 接口见 tools/dlp-egress/docs/INTERFACE.md）：
//! - DLP 复用 `[egress]` 的外发检查守卫（mode 不是 off 时才有），不另起进程；无论 audit 还是 enforce，
//!   这里都只在 `ready_to_send` 时发送；
//! - 第 3 层诊断包：先做本机确定性处理（`an_agents::diagnose::scrub_local`）；配置了 DLP 时再经它检查
//!   （渠道 clipboard），用完即撤销。没配置或 DLP 不可用时照样显示，并注明"未经外发脱敏检查"——
//!   这一步系统不往外发，由用户自己决定复制给谁；
//! - 第 4 层：必须配置了 DLP。预览时评估（渠道 agent、目的地 `agent:<帮手>`），只有 `ready_to_send`
//!   才生成预览，预览内容就是将发出的全部内容；同意凭证一次有效、5 分钟过期，发送时用服务端保存的
//!   那份内容（不接受界面传回内容）；
//! - 帮手用自己的配置另起临时实例：权限一律拒绝、空的临时工作目录、不进任务队列；全局同时 1 个；
//! - 帮手回复里的占位符用 DLP 还原（context = local_ui），只用于显示；结束后撤销这批映射。
//!
//! 超时、取消与清理：帮手单次最多 [`HELPER_TIMEOUT`]（测试可调短）。超时或请求被取消时，执行的 future
//! 被丢弃，ACP SDK 结束帮手的整个进程组；临时目录在本函数结束时删除（取消时由 [`TempDir`] 的 Drop 删除）。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_agents::acp::PermissionMode;
use an_agents::egress::EgressGuard;
use an_agents::AcpAgent;
use an_core::{
    AgentAdapter, DiagnosePackage, DiagnosePreview, DiagnoseSendResult, EventPublisher, EventSink,
    Risk, Role, RouteDecision, TaskClass, TaskEvent, TaskSpec,
};

use crate::{Prober, Runtime};

/// 同意凭证的有效期
pub const CONSENT_TTL: Duration = Duration::from_secs(300);
/// 帮手单次运行的时限
pub const HELPER_TIMEOUT: Duration = Duration::from_secs(120);

/// 给帮手的说明文字（固定，放在待发内容最前面）
const INSTRUCTIONS: &str = "你是排查助手。下面是另一个助手接入 AgentNexus 失败时的诊断信息，\
其中一部分内容已经替换成占位符（例如 [CUSTOMER_001]、<user>），请保持占位符原样。\
请用简体中文给出最可能的原因，以及一步一步的修复办法。不要修改任何文件，也不要运行命令。";

/// 一次待用的同意
struct Consent {
    token: String,
    /// 将发送的完整内容（服务端保存，发送时只用这一份）
    content: String,
    /// DLP 映射的作用域；用完或过期都要撤销
    task_id: String,
    helper: String,
    created: Instant,
    /// 测试用：强制视为已过期
    expired: bool,
}

impl Consent {
    fn is_expired(&self) -> bool {
        self.expired || self.created.elapsed() > CONSENT_TTL
    }
}

/// 第 4 层的状态：同意凭证、全局并发闸门（同时 1 个）、帮手时限
pub struct SendState {
    consents: Mutex<HashMap<String, Consent>>,
    gate: Arc<tokio::sync::Semaphore>,
    helper_timeout: Mutex<Duration>,
}

impl Default for SendState {
    fn default() -> Self {
        Self {
            consents: Mutex::default(),
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            helper_timeout: Mutex::new(HELPER_TIMEOUT),
        }
    }
}

/// 第 4 层的错误：种类决定 HTTP 状态码，`message` 是给用户的大白话（技术信息放在最后的括号里）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnoseSendError {
    NotFound(String),
    Conflict(String),
    BadToken(String),
    Expired(String),
    Timeout(String),
    Failed(String),
}

impl std::fmt::Display for DiagnoseSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use DiagnoseSendError::*;
        match self {
            NotFound(m) | Conflict(m) | BadToken(m) | Expired(m) | Timeout(m) | Failed(m) => {
                f.write_str(m)
            }
        }
    }
}

/// 帮手的进度事件不进任何会话
struct NoopPublisher;
impl EventPublisher for NoopPublisher {
    fn publish(&self, _event: TaskEvent) {}
}

/// 临时工作目录：离开作用域（包括请求被取消）时删除
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 错误说明只保留前 120 个字符
fn short_err(e: &anyhow::Error) -> String {
    an_core::truncate_chars(&format!("{e}"), 120)
}

/// 待发内容的摘要（只用于审计日志关联，不是加密摘要）
fn digest(text: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

impl Runtime {
    fn dlp_client(&self) -> Option<&Arc<EgressGuard>> {
        self.egress.as_ref()
    }

    /// 第 3 层：诊断包。本机处理过的原文 → 配置了 DLP 时再经它检查（渠道 clipboard），用完即撤销
    pub(crate) async fn diagnose_package(&self, local_text: String) -> DiagnosePackage {
        const LOCAL_ONLY: &str = "只在本机做了处理（家目录换成 ~，用户名、主机名已替换，环境变量只留名字）。复制给别人前请再看一遍。";
        let Some(dlp) = self.dlp_client() else {
            return DiagnosePackage {
                text: Some(local_text),
                dlp_checked: false,
                note: format!(
                    "未经外发脱敏检查：没有开启外发检查（config.toml 的 [egress]）。{LOCAL_ONLY}"
                ),
            };
        };
        let task_id = an_core::new_id("diag-pkg");
        let result = dlp
            .evaluate_text(
                &task_id,
                "clipboard",
                "user:clipboard",
                "diagnose",
                &local_text,
            )
            .await;
        let package = match result {
            Ok(e) if e.ready_to_send => DiagnosePackage {
                text: e.sanitized_text.clone(),
                dlp_checked: true,
                note: "已经过外发脱敏检查：敏感信息已换成占位符（例如 [CUSTOMER_001]）。".into(),
            },
            // 工具还不支持 clipboard 渠道、或没有登记 user:clipboard：属于配置问题，不是内容问题，
            // 按"没有经过检查"处理（第 3 层不外发，照样给本机处理过的版本）
            Ok(e) if e.reason_codes.iter().all(|c| {
                matches!(
                    c.as_str(),
                    "invalid_channel" | "destination_unknown" | "purpose_not_allowed"
                )
            }) && e.decision == "BLOCK" =>
            {
                DiagnosePackage {
                    text: Some(local_text),
                    dlp_checked: false,
                    note: format!(
                        "未经外发脱敏检查：外发检查工具还不支持或没有登记诊断包这个去向（{}）。{LOCAL_ONLY}",
                        e.reason_codes.join("、")
                    ),
                }
            }
            Ok(e) => DiagnosePackage {
                text: None,
                dlp_checked: true,
                note: format!("{}诊断包不提供复制。", e.refusal()),
            },
            Err(err) => DiagnosePackage {
                text: Some(local_text),
                dlp_checked: false,
                note: format!("未经外发脱敏检查：外发检查暂时不可用（{}）。{LOCAL_ONLY}", short_err(&err)),
            },
        };
        // 交给用户复制的是脱敏文本，之后不需要还原：立即撤销映射
        dlp.revoke(&task_id).await;
        package
    }

    /// 第 4 层：生成预览（将发送的完整内容 + 目的地说明 + 一次性同意凭证）。这一步不发送任何东西
    pub async fn diagnose_preview(
        &self,
        id: &str,
        helper: &str,
    ) -> Result<DiagnosePreview, DiagnoseSendError> {
        use DiagnoseSendError::*;
        self.purge_expired_consents().await;
        // 先校验帮手，再做任何评估
        if helper == id {
            return Err(Conflict(
                "不能请出问题的助手自己帮忙查，请换一个已连通的助手。（技术信息：E_DIAG_HELPER_SELF）".into(),
            ));
        }
        let (_, helper_name) = self.helper_config(helper)?;
        let Some(dlp) = self.dlp_client().cloned() else {
            return Err(Conflict(
                "没有配置外发脱敏（config.toml 的 [egress]，mode 需为 audit 或 enforce），不能把内容发给其他助手。可以复制诊断包，自己决定怎么用。（技术信息：E_DLP_UNCONFIGURED）".into(),
            ));
        };
        let Some((_, local_text)) = self.diagnose_core(id).await else {
            return Err(NotFound(format!("可排查的 ACP agent `{id}` 不存在")));
        };
        let task_id = an_core::new_id("diag");
        let evaluation = dlp
            .evaluate_text(
                &task_id,
                "agent",
                &format!("agent:{helper}"),
                "diagnose",
                &local_text,
            )
            .await
            .map_err(|e| {
                Conflict(format!(
                    "外发脱敏检查暂时不可用（{}），这次不能发送。（技术信息：E_DLP_UNAVAILABLE）",
                    short_err(&e)
                ))
            })?;
        let sanitized = match (&evaluation.sanitized_text, evaluation.ready_to_send) {
            (Some(text), true) => text.clone(),
            _ => {
                dlp.revoke(&task_id).await;
                return Err(Conflict(format!(
                    "{}（技术信息：E_DLP_REFUSED {}）",
                    evaluation.refusal(),
                    evaluation.decision
                )));
            }
        };
        let content = format!("{INSTRUCTIONS}\n\n{sanitized}");
        let consent_id = an_core::new_id("consent");
        let token = an_core::new_id("tok");
        self.diagnose_send
            .consents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                consent_id.clone(),
                Consent {
                    token: token.clone(),
                    content: content.clone(),
                    task_id,
                    helper: helper.to_string(),
                    created: Instant::now(),
                    expired: false,
                },
            );
        Ok(DiagnosePreview {
            consent_id,
            consent_token: token,
            content,
            destination: format!(
                "将发给「{helper_name}」。它会把这段内容发给它自己使用的模型服务，之后怎么处理不在本系统的控制范围内。"
            ),
            expires_in_secs: CONSENT_TTL.as_secs(),
        })
    }

    /// 第 4 层：凭同意凭证发送一次。发送的是服务端保存的那份内容
    pub async fn diagnose_send(
        &self,
        consent_id: &str,
        token: &str,
    ) -> Result<DiagnoseSendResult, DiagnoseSendError> {
        use DiagnoseSendError::*;
        // 校验凭证；全局同一时间只处理一次（忙时凭证保留，可稍后再用）。
        // 锁只在这个块里持有，不跨 await
        enum Check {
            Go(Consent, tokio::sync::OwnedSemaphorePermit),
            Expired(String),
            Refuse(DiagnoseSendError),
        }
        let check = {
            let mut consents = self
                .diagnose_send
                .consents
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match consents.get(consent_id) {
                None => Check::Refuse(NotFound(
                    "这次同意不存在或已经用过，请重新生成预览。（技术信息：E_DIAG_CONSENT_UNKNOWN）".into(),
                )),
                Some(c) if c.token != token => Check::Refuse(BadToken(
                    "同意凭证不对，没有发送。（技术信息：E_DIAG_CONSENT_TOKEN）".into(),
                )),
                Some(c) if c.is_expired() => {
                    let task_id = c.task_id.clone();
                    consents.remove(consent_id);
                    Check::Expired(task_id)
                }
                Some(_) => match self.diagnose_send.gate.clone().try_acquire_owned() {
                    Ok(permit) => {
                        Check::Go(consents.remove(consent_id).expect("checked"), permit)
                    }
                    Err(_) => Check::Refuse(Conflict(
                        "上一次发送还没结束，请稍后再试。（技术信息：E_DIAG_BUSY）".into(),
                    )),
                },
            }
        };
        let (consent, _permit) = match check {
            Check::Go(c, p) => (c, p),
            Check::Refuse(e) => return Err(e),
            Check::Expired(task_id) => {
                if let Some(dlp) = self.dlp_client() {
                    dlp.revoke(&task_id).await;
                }
                return Err(Expired(
                    "这次同意已经过期，没有发送，请重新生成预览。（技术信息：E_DIAG_CONSENT_EXPIRED）".into(),
                ));
            }
        };
        let dlp = self.dlp_client().cloned();
        let result = self.run_helper(&consent).await;
        // 无论成功、失败还是超时，都撤销这批映射（成功时先还原回复再撤销）
        let outcome = match result {
            Ok((helper_name, reply)) => {
                let (reply, restored) = match &dlp {
                    Some(d) => match d.restore_text(&consent.task_id, &reply).await {
                        Ok(text) => (text, true),
                        Err(_) => (reply, false),
                    },
                    None => (reply, false),
                };
                Ok(DiagnoseSendResult {
                    helper: helper_name,
                    reply,
                    restored,
                })
            }
            Err(e) => Err(e),
        };
        if let Some(d) = &dlp {
            d.revoke(&consent.task_id).await;
        }
        outcome
    }

    /// 启动帮手的临时实例跑一次：权限一律拒绝、空的临时工作目录、不进任务队列
    async fn run_helper(&self, consent: &Consent) -> Result<(String, String), DiagnoseSendError> {
        use DiagnoseSendError::*;
        let (mut cfg, helper_name) = self.helper_config(&consent.helper)?;
        cfg.permission = PermissionMode::DenyAll;
        let dir = TempDir(std::env::temp_dir().join(an_core::new_id("an-diag-helper")));
        std::fs::create_dir_all(&dir.0)
            .map_err(|e| Failed(format!("无法创建临时目录：{e}（技术信息：E_DIAG_TEMPDIR）")))?;
        cfg.workdir = Some(dir.0.clone());
        let agent = AcpAgent::new(cfg, dir.0.clone());
        let task = TaskSpec {
            task_id: consent.task_id.clone(),
            conversation_id: String::new(),
            goal: consent.content.clone(),
            context: String::new(),
            route: RouteDecision {
                task_class: TaskClass::L1,
                execution_role: Role::General,
                required_tools: vec![],
                risk_level: Risk::Low,
                reason: "AG.5 排查：请帮手只读排查".into(),

                template_id: String::new(),
                deliverable: String::new(),
                slots_ok: String::new(),
            },
        };
        // 记录外发：目的地、内容摘要和长度，不记内容本身
        tracing::info!(
            event = "agent.diagnose_sent",
            task_id = %consent.task_id,
            helper = %consent.helper,
            digest = %digest(&consent.content),
            content_len = consent.content.chars().count(),
            "排查内容已发给帮手"
        );
        let sink = EventSink::new(Arc::new(NoopPublisher), "", &consent.task_id);
        let limit = *self
            .diagnose_send
            .helper_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let run = tokio::time::timeout(limit, agent.run(task, sink)).await;
        drop(dir);
        match run {
            Err(_) => Err(Timeout(format!(
                "「{helper_name}」没有在 {} 秒内回复，已经结束它。（技术信息：E_DIAG_HELPER_TIMEOUT）",
                limit.as_secs()
            ))),
            Ok(Err(e)) => Err(Failed(format!(
                "「{helper_name}」运行失败：{e}（技术信息：E_DIAG_HELPER_FAILED）"
            ))),
            Ok(Ok(result)) => {
                let mut reply = result.summary;
                if !result.errors.is_empty() {
                    reply.push_str("\n（帮手没有正常结束：");
                    reply.push_str(&result.errors.join("；"));
                    reply.push('）');
                }
                Ok((helper_name, reply))
            }
        }
    }

    /// 帮手的配置：必须是已注册、已启用、已连通的 ACP 助手
    fn helper_config(
        &self,
        helper: &str,
    ) -> Result<(an_agents::acp::AcpConfig, String), DiagnoseSendError> {
        use DiagnoseSendError::*;
        let cfg = self
            .probers
            .snapshot()
            .into_iter()
            .find_map(|p| match p {
                Prober::Acp(a) if AgentAdapter::card(a.as_ref()).id == helper => {
                    Some(a.config().clone())
                }
                _ => None,
            })
            .ok_or_else(|| NotFound(format!("可当帮手的助手 `{helper}` 不存在")))?;
        let name = cfg.name.clone().unwrap_or_else(|| cfg.id.clone());
        let card = self.registry.cards().into_iter().find(|c| c.id == helper);
        let enabled = card.as_ref().is_some_and(|c| c.enabled);
        let state = self.registry.links.state(helper);
        let status = crate::link::link_status(Instant::now(), enabled, &state);
        if status != crate::link::LinkStatus::Connected {
            return Err(Conflict(format!(
                "「{name}」还没有连通，不能当帮手。可以先在「助手」页签里检测它。（技术信息：E_DIAG_HELPER_OFFLINE）"
            )));
        }
        Ok((cfg, name))
    }

    /// 清理过期的同意，并撤销它们的映射
    async fn purge_expired_consents(&self) {
        let expired: Vec<String> = {
            let mut consents = self
                .diagnose_send
                .consents
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let ids: Vec<String> = consents
                .iter()
                .filter(|(_, c)| c.is_expired())
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| consents.remove(&id).map(|c| c.task_id))
                .collect()
        };
        if let Some(dlp) = self.dlp_client() {
            for task_id in expired {
                dlp.revoke(&task_id).await;
            }
        }
    }

    /// 测试用：让所有未使用的同意凭证立即过期
    #[doc(hidden)]
    pub fn expire_diagnose_consents(&self) {
        for c in self
            .diagnose_send
            .consents
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
        {
            c.expired = true;
        }
    }

    /// 测试用：调整帮手单次运行的时限（默认 [`HELPER_TIMEOUT`]）
    #[doc(hidden)]
    pub fn set_diagnose_helper_timeout(&self, limit: Duration) {
        *self
            .diagnose_send
            .helper_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = limit;
    }
}
