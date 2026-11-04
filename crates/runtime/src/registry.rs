//! Agent 注册表：Router 只能从这里选角色，模型不能自创 Agent。

use std::collections::BTreeMap;
use std::sync::Arc;

use an_core::{AgentAdapter, AgentCard, Role};

#[derive(Default, Clone)]
pub struct AgentRegistry {
    agents: BTreeMap<&'static str, (Role, Arc<dyn AgentAdapter>)>,
}

impl AgentRegistry {
    pub fn register(&mut self, agent: Arc<dyn AgentAdapter>) {
        let role = agent.card().role;
        self.agents.insert(role.as_str(), (role, agent));
    }

    pub fn get(&self, role: Role) -> Option<Arc<dyn AgentAdapter>> {
        self.agents.get(role.as_str()).map(|(_, a)| a.clone())
    }

    /// 所有已注册 Agent 的能力卡片（包括不可用的，前端展示用）
    pub fn cards(&self) -> Vec<AgentCard> {
        self.agents.values().map(|(_, a)| a.card().clone()).collect()
    }

    /// 当前可用的角色
    pub fn enabled_roles(&self) -> Vec<Role> {
        self.agents.values().filter(|(_, a)| a.card().enabled).map(|(r, _)| *r).collect()
    }

    pub fn is_enabled(&self, role: Role) -> bool {
        self.get(role).map(|a| a.card().enabled).unwrap_or(false)
    }
}
