//! Agent 注册表。
//!
//! - **角色**（general / coding / reviewer）是"做什么"，Router 只选角色；
//! - **agent** 是"谁来做"，同一角色可以注册多个（内置、Codex CLI、各种 ACP agent）。
//!
//! 选择规则（极简、可预测）：
//! 1. 只考虑可用的 agent（已安装）；
//! 2. 配置了首选（`[agents.prefer]`）的排第一，其余按注册顺序；
//! 3. 排第一的 agent 启动失败时，编排层自动换下一个（任务执行中失败不换）。

use std::collections::BTreeMap;
use std::sync::Arc;

use an_core::{AgentAdapter, AgentCard, Role};

#[derive(Default, Clone)]
pub struct AgentRegistry {
    /// 按注册顺序保存
    agents: Vec<Arc<dyn AgentAdapter>>,
    /// 角色 → 首选 agent id
    prefer: BTreeMap<String, String>,
}

impl AgentRegistry {
    pub fn new(prefer: BTreeMap<String, String>) -> Self {
        Self { agents: vec![], prefer }
    }

    /// 注册 agent；id 重复时忽略后注册的那个（自定义配置优先于预设）
    pub fn register(&mut self, agent: Arc<dyn AgentAdapter>) -> bool {
        if self.agents.iter().any(|a| a.card().id == agent.card().id) {
            return false;
        }
        self.agents.push(agent);
        true
    }

    /// 某角色的候选 agent（只含可用的），首选在前
    pub fn candidates(&self, role: Role) -> Vec<Arc<dyn AgentAdapter>> {
        let mut list: Vec<_> = self.agents.iter().filter(|a| a.card().role == role && a.card().enabled).cloned().collect();
        if let Some(pref) = self.prefer.get(role.as_str()) {
            // 稳定排序：首选排到最前，其余保持注册顺序
            list.sort_by_key(|a| a.card().id != *pref);
        }
        list
    }

    /// 某角色当前会被使用的 agent
    pub fn get(&self, role: Role) -> Option<Arc<dyn AgentAdapter>> {
        self.candidates(role).into_iter().next()
    }

    pub fn is_enabled(&self, role: Role) -> bool {
        self.get(role).is_some()
    }

    /// 有可用 agent 的角色
    pub fn enabled_roles(&self) -> Vec<Role> {
        Role::ALL.into_iter().filter(|r| self.is_enabled(*r)).collect()
    }

    /// 所有 agent 的能力卡片（包括不可用的，前端展示用）
    pub fn cards(&self) -> Vec<AgentCard> {
        self.agents.iter().map(|a| a.card().clone()).collect()
    }

    /// 某角色所有 agent 不可用的原因（Policy 拒绝路由时告诉用户）
    pub fn unavailable_reasons(&self, role: Role) -> Vec<String> {
        self.agents
            .iter()
            .filter(|a| a.card().role == role)
            .map(|a| format!("{}：{}", a.card().name, a.card().disabled_reason.clone().unwrap_or_else(|| "不可用".into())))
            .collect()
    }

    pub fn prefer(&self) -> &BTreeMap<String, String> {
        &self.prefer
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
    fn fake(id: &str, role: Role, enabled: bool) -> Arc<dyn AgentAdapter> {
        Arc::new(Fake(AgentCard {
            id: id.into(),
            kind: "test".into(),
            role,
            name: id.into(),
            description: String::new(),
            tools: vec![],
            enabled,
            disabled_reason: None,
        }))
    }

    #[test]
    fn prefer_and_order() {
        let mut r = AgentRegistry::new([("coding".to_string(), "opencode".to_string())].into());
        r.register(fake("codex", Role::Coding, true));
        r.register(fake("pi", Role::Coding, false));
        r.register(fake("opencode", Role::Coding, true));
        assert!(!r.register(fake("codex", Role::Coding, true)), "重复 id 应被忽略");
        let ids: Vec<_> = r.candidates(Role::Coding).iter().map(|a| a.card().id.clone()).collect();
        assert_eq!(ids, ["opencode", "codex"]); // 首选在前，不可用的被排除
        assert!(!r.is_enabled(Role::Reviewer));
    }
}
