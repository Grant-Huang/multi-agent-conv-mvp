//! Agent 注册表与执行组。
//!
//! 概念（用大白话说）：
//! - **角色**（general / coding / reviewer）= 要做什么事。Router 只选角色。
//! - **agent** = 谁来做。可以是内置的、本机的（Codex、ACP），也可以是远程节点上的。
//! - **执行组** = 某个角色由哪几个 agent 来做。组里第一个是主力，后面是待命的备份。
//!
//! 规则：
//! 1. 执行组只能由配置**明确指定**（`[agents.assign] coding = ["hermes@mac-a", "hermes@mac-b"]`），
//!    系统不会根据能力卡片自己去找别的 agent；
//! 2. 组内成员必须是**同一类型**（agent_type 相同，例如都是 hermes），否则报错 E_POOL_CONFIG；
//! 3. 备份只用于**分担并发**：主力忙满了（达到 max_concurrent），新任务交给空闲的备份；
//!    全都忙就排队等待；离线的成员跳过；
//! 4. 任务执行**出错就报错**，不会换给别的 agent 重做；
//! 5. 没有配置执行组的角色：使用该角色第一个可用的 agent（注册顺序：内置 → Codex → 本机 ACP → 节点）。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use an_core::{codes, AgentAdapter, AgentCard, AgentError, Role};
use tokio::sync::Notify;

struct Entry {
    agent: Arc<dyn AgentAdapter>,
    /// 正在执行的任务数
    busy: u32,
    /// 来自哪个节点（本机 agent 为 None）
    node_id: Option<String>,
}

struct Inner {
    /// 按注册顺序保存
    agents: Vec<Entry>,
    /// 角色 → 执行组成员 id（按优先顺序）
    assign: BTreeMap<String, Vec<String>>,
}

pub struct AgentRegistry {
    inner: Mutex<Inner>,
    /// 有 agent 空闲出来或 agent 列表变化时通知排队的任务
    changed: Notify,
    /// 连接状态（绿 / 灰 / 红）：成功调用与后台探测写入，健康接口读取
    pub links: crate::link::LinkRegistry,
}

/// 占用一个 agent 的执行名额；drop 时自动归还
pub struct Lease {
    registry: Arc<AgentRegistry>,
    pub agent: Arc<dyn AgentAdapter>,
    /// 在执行组可用成员中的位置：0 = 主力，之后是备份
    pub member_index: usize,
    /// 分配时执行组的可用成员数
    pub group_size: usize,
    /// 从开始申请到拿到名额等了多久（没排队时接近 0）
    pub waited: Duration,
}

/// agent 给人看的名称："名称（接入方式，位置）"；节点 agent 的名称里已带机器名，只补接入方式
pub fn display_name(card: &AgentCard) -> String {
    if card.kind == "node" {
        format!("{}（{}）", card.name, card.kind)
    } else {
        format!("{}（{}，{}）", card.name, card.kind, card.location)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let id = self.agent.card().id.clone();
        {
            let mut inner = self.registry.lock();
            if let Some(e) = inner.agents.iter_mut().find(|e| e.agent.card().id == id) {
                e.busy = e.busy.saturating_sub(1);
            }
        }
        self.registry.changed.notify_waiters();
    }
}

impl AgentRegistry {
    pub fn new(assign: BTreeMap<String, Vec<String>>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                agents: vec![],
                assign,
            }),
            changed: Notify::new(),
            links: Default::default(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 注册本机 agent；id 重复时忽略后注册的（自定义配置优先于预设）
    pub fn register(&self, agent: Arc<dyn AgentAdapter>) -> bool {
        let mut inner = self.lock();
        if inner
            .agents
            .iter()
            .any(|e| e.agent.card().id == agent.card().id)
        {
            return false;
        }
        inner.agents.push(Entry {
            agent,
            busy: 0,
            node_id: None,
        });
        true
    }

    /// 用最新的列表替换某个节点的全部 agent（节点上线、下线、agent 变化时调用）。
    /// 正在执行的任务数会保留，执行中的任务不受影响。
    pub fn replace_node_agents(&self, node_id: &str, agents: Vec<Arc<dyn AgentAdapter>>) {
        {
            let mut inner = self.lock();
            let old_busy: BTreeMap<String, u32> = inner
                .agents
                .iter()
                .filter(|e| e.node_id.as_deref() == Some(node_id))
                .map(|e| (e.agent.card().id.clone(), e.busy))
                .collect();
            inner
                .agents
                .retain(|e| e.node_id.as_deref() != Some(node_id));
            for agent in agents {
                let busy = old_busy.get(&agent.card().id).copied().unwrap_or(0);
                inner.agents.push(Entry {
                    agent,
                    busy,
                    node_id: Some(node_id.to_string()),
                });
            }
        }
        self.changed.notify_waiters();
    }

    /// 移除某个节点的全部 agent（解除配对时）
    pub fn remove_node(&self, node_id: &str) {
        self.replace_node_agents(node_id, vec![]);
    }

    /// 解析某角色的执行组：返回成员（按优先顺序，含不可用的），或配置错误
    fn group(inner: &Inner, role: Role) -> Result<Vec<&Entry>, AgentError> {
        let Some(ids) = inner.assign.get(role.as_str()) else {
            // 没有配置执行组：第一个可用的；都不可用时返回第一个（用于给出原因）
            let of_role = inner.agents.iter().filter(|e| e.agent.card().role == role);
            let first = of_role
                .clone()
                .find(|e| e.agent.card().enabled)
                .or_else(|| of_role.clone().next());
            return Ok(first.into_iter().collect());
        };
        let mut members = vec![];
        for id in ids {
            match inner.agents.iter().find(|e| &e.agent.card().id == id) {
                Some(e) => members.push(e),
                // 可能是还没连上的节点上的 agent，跳过但留下记录
                None => {
                    tracing::debug!(event = "pool.member_missing", code = codes::POOL_MEMBER_MISSING, role = role.as_str(), agent_id = %id)
                }
            }
        }
        // 校验：角色一致、类型一致
        if let Some(bad) = members.iter().find(|e| e.agent.card().role != role) {
            return Err(AgentError::new(
                codes::POOL_CONFIG,
                format!(
                    "执行组 {} 中的 {} 角色是 {}，不能放进这个组",
                    role.as_str(),
                    bad.agent.card().id,
                    bad.agent.card().role.as_str()
                ),
            ));
        }
        if let Some(first) = members.first() {
            let ty = &first.agent.card().agent_type;
            if let Some(bad) = members.iter().find(|e| &e.agent.card().agent_type != ty) {
                return Err(AgentError::new(
                    codes::POOL_CONFIG,
                    format!(
                        "执行组 {} 只能放同一类型的 agent：{} 是 {}，{} 是 {}",
                        role.as_str(),
                        first.agent.card().id,
                        ty,
                        bad.agent.card().id,
                        bad.agent.card().agent_type
                    ),
                ));
            }
        }
        Ok(members)
    }

    /// 某角色执行组的成员 id（按优先顺序，含不可用的）
    pub fn group_ids(&self, role: Role) -> Result<Vec<String>, AgentError> {
        let inner = self.lock();
        Self::group(&inner, role).map(|g| g.iter().map(|e| e.agent.card().id.clone()).collect())
    }

    /// 某角色当前是否有可用的 agent
    pub fn is_enabled(&self, role: Role) -> bool {
        let inner = self.lock();
        Self::group(&inner, role)
            .map(|g| g.iter().any(|e| e.agent.card().enabled))
            .unwrap_or(false)
    }

    /// 某角色执行组里当前可用的成员（显示名，按组内顺序，主力在前）。执行组配置错误时为空
    pub fn candidates(&self, role: Role) -> Vec<String> {
        let inner = self.lock();
        Self::group(&inner, role)
            .map(|g| {
                g.iter()
                    .filter(|e| e.agent.card().enabled)
                    .map(|e| display_name(e.agent.card()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 有可用 agent 的角色
    pub fn enabled_roles(&self) -> Vec<Role> {
        Role::ALL
            .into_iter()
            .filter(|r| self.is_enabled(*r))
            .collect()
    }

    /// 所有 agent 的能力卡片（包括不可用的，前端展示用）
    pub fn cards(&self) -> Vec<AgentCard> {
        self.lock()
            .agents
            .iter()
            .map(|e| e.agent.card().clone())
            .collect()
    }

    /// 某角色不可用的原因（Policy 拒绝路由时告诉用户）
    pub fn unavailable_reasons(&self, role: Role) -> Vec<String> {
        let inner = self.lock();
        match Self::group(&inner, role) {
            Err(e) => vec![e.to_string()],
            Ok(g) if g.is_empty() => vec![format!("没有 {} 角色的 agent", role.as_str())],
            Ok(g) => g
                .iter()
                .map(|e| {
                    format!(
                        "{}：{}",
                        e.agent.card().name,
                        e.agent
                            .card()
                            .disabled_reason
                            .clone()
                            .unwrap_or_else(|| "不可用".into())
                    )
                })
                .collect(),
        }
    }

    /// 为某角色分配一个 agent。组内有空闲的立即返回；都在忙就排队等待（`on_wait` 只在第一次排队时调用）。
    pub async fn acquire(
        self: &Arc<Self>,
        role: Role,
        on_wait: impl Fn(String),
    ) -> Result<Lease, AgentError> {
        let started = Instant::now();
        let mut waited = false;
        loop {
            // 先登记通知再检查，避免"检查完、开始等之前"刚好有 agent 空出来而错过通知
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut inner = self.lock();
                let ids: Vec<String> = Self::group(&inner, role)?
                    .iter()
                    .filter(|e| e.agent.card().enabled)
                    .map(|e| e.agent.card().id.clone())
                    .collect();
                if ids.is_empty() {
                    drop(inner);
                    return Err(AgentError::new(
                        codes::ROUTE_NO_AGENT,
                        format!(
                            "没有可用的 {} agent：{}",
                            role.as_str(),
                            self.unavailable_reasons(role).join("；")
                        ),
                    ));
                }
                let total = ids.len();
                // 按组内顺序找第一个有空闲名额的
                for (index, id) in ids.iter().enumerate() {
                    let e = inner
                        .agents
                        .iter_mut()
                        .find(|e| &e.agent.card().id == id)
                        .expect("刚解析出的成员");
                    if e.busy < e.agent.card().max_concurrent {
                        e.busy += 1;
                        return Ok(Lease {
                            registry: self.clone(),
                            agent: e.agent.clone(),
                            member_index: index,
                            group_size: total,
                            waited: started.elapsed(),
                        });
                    }
                }
                if !waited {
                    waited = true;
                    tracing::warn!(
                        event = "pool.wait",
                        code = codes::POOL_WAIT,
                        role = role.as_str(),
                        members = total,
                        "执行组的 agent 都在忙，任务排队等待"
                    );
                    on_wait(format!(
                        "{} 个 {} agent 都在忙，排队等待空闲",
                        total,
                        role.as_str()
                    ));
                }
            }
            // 有 agent 空闲或列表变化时醒来；另外每 5 秒兜底检查一次（例如节点重新上线）
            let _ = tokio::time::timeout(Duration::from_secs(5), notified).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_core::{EventSink, TaskResult, TaskSpec};

    struct Fake(AgentCard);
    #[async_trait::async_trait]
    impl AgentAdapter for Fake {
        fn card(&self) -> &AgentCard {
            &self.0
        }
        async fn run(&self, _: TaskSpec, _: EventSink) -> anyhow::Result<TaskResult> {
            unreachable!()
        }
    }
    fn fake(id: &str, ty: &str, role: Role, enabled: bool, max: u32) -> Arc<dyn AgentAdapter> {
        Arc::new(Fake(AgentCard {
            id: id.into(),
            kind: "test".into(),
            agent_type: ty.into(),
            location: "本机".into(),
            max_concurrent: max,
            role,
            name: id.into(),
            description: String::new(),
            tools: vec![],
            enabled,
            disabled_reason: (!enabled).then(|| "离线".into()),
        }))
    }
    fn assign(role: &str, ids: &[&str]) -> BTreeMap<String, Vec<String>> {
        [(
            role.to_string(),
            ids.iter().map(|s| s.to_string()).collect(),
        )]
        .into()
    }

    #[test]
    fn default_group_is_first_enabled() {
        let r = AgentRegistry::new(BTreeMap::new());
        r.register(fake("codex", "codex", Role::Coding, false, 1));
        r.register(fake("opencode", "opencode", Role::Coding, true, 1));
        assert!(
            !r.register(fake("codex", "codex", Role::Coding, true, 1)),
            "重复 id 应被忽略"
        );
        assert_eq!(r.group_ids(Role::Coding).unwrap(), ["opencode"]);
        assert!(!r.is_enabled(Role::Reviewer));
    }

    #[test]
    fn group_rejects_mixed_types() {
        let r = AgentRegistry::new(assign("coding", &["opencode", "codex"]));
        r.register(fake("codex", "codex", Role::Coding, true, 1));
        r.register(fake("opencode", "opencode", Role::Coding, true, 1));
        let e = r.group_ids(Role::Coding).unwrap_err();
        assert_eq!(e.code, codes::POOL_CONFIG);
        assert!(!r.is_enabled(Role::Coding), "配置错误时不应被当作可用");
    }

    #[test]
    fn group_rejects_wrong_role() {
        let r = AgentRegistry::new(assign("coding", &["hermes"]));
        r.register(fake("hermes", "hermes", Role::General, true, 1));
        assert_eq!(
            r.group_ids(Role::Coding).unwrap_err().code,
            codes::POOL_CONFIG
        );
    }

    #[tokio::test]
    async fn backups_share_load_and_queue() {
        let r = AgentRegistry::new(assign("general", &["hermes@a", "hermes@b", "hermes@c"]));
        r.register(fake("hermes@a", "hermes", Role::General, true, 1));
        r.register(fake("hermes@b", "hermes", Role::General, true, 1));
        r.register(fake("hermes@c", "hermes", Role::General, false, 1)); // 离线：跳过
        let l1 = r.acquire(Role::General, |_| {}).await.unwrap();
        let l2 = r.acquire(Role::General, |_| {}).await.unwrap();
        assert_eq!(l1.agent.card().id, "hermes@a", "主力优先");
        assert_eq!(l2.agent.card().id, "hermes@b", "主力忙时用备份");

        // 第三个任务应排队，直到有人释放
        let waits = Arc::new(Mutex::new(vec![]));
        let (r2, w2) = (r.clone(), waits.clone());
        let third = tokio::spawn(async move {
            r2.acquire(Role::General, move |m| w2.lock().unwrap().push(m))
                .await
                .unwrap()
                .agent
                .card()
                .id
                .clone()
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!third.is_finished(), "都在忙时应排队");
        assert_eq!(waits.lock().unwrap().len(), 1);
        drop(l1);
        assert_eq!(
            third.await.unwrap(),
            "hermes@a",
            "释放后排队的任务拿到空出的 agent"
        );
    }

    #[tokio::test]
    async fn node_agents_replace_keeps_busy() {
        let r = AgentRegistry::new(assign("general", &["hermes@a"]));
        r.replace_node_agents(
            "node-a",
            vec![fake("hermes@a", "hermes", Role::General, true, 1)],
        );
        let lease = r.acquire(Role::General, |_| {}).await.unwrap();
        // 心跳刷新不影响正在执行的任务计数
        r.replace_node_agents(
            "node-a",
            vec![fake("hermes@a", "hermes", Role::General, true, 1)],
        );
        let waiting = {
            let r = r.clone();
            tokio::spawn(async move { r.acquire(Role::General, |_| {}).await.is_ok() })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiting.is_finished(), "名额仍被占用");
        drop(lease);
        assert!(waiting.await.unwrap());
        // 节点解除配对后没有可用 agent
        r.remove_node("node-a");
        assert_eq!(
            r.acquire(Role::General, |_| {}).await.err().unwrap().code,
            codes::ROUTE_NO_AGENT
        );
    }
}
