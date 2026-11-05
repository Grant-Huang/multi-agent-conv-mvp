//! Policy：确定性规则，不调用模型。
//! "模型负责理解，程序负责决策"——Router 模型给出的风险等级只是建议，
//! 是否自动执行、是否需要审查由这里的规则决定。

use an_core::{PolicyVerdict, Risk, Role, RouteDecision, TaskClass};

use crate::registry::AgentRegistry;

pub struct Policy {
    /// 风险达到该等级需要人工审批
    pub approval_risk: Risk,
}

impl Policy {
    /// 校验路由决策并给出裁决。返回 Err 表示路由不可执行（例如角色不可用）。
    pub fn evaluate(&self, route: &mut RouteDecision, registry: &AgentRegistry) -> Result<PolicyVerdict, String> {
        let mut notes = vec![];

        // 规则 1：角色必须至少有一个可用的 agent
        if !registry.is_enabled(route.execution_role) {
            let reasons = registry.unavailable_reasons(route.execution_role);
            tracing::warn!(event = "policy.reject", code = an_core::codes::ROUTE_NO_AGENT, role = route.execution_role.as_str(), reasons = %reasons.join("；"), "没有可用的 agent，拒绝路由");
            return Err(format!(
                "[{}] 没有可用的 {} agent{}",
                an_core::codes::ROUTE_NO_AGENT,
                route.execution_role.as_str(),
                if reasons.is_empty() { String::new() } else { format!("（{}）", reasons.join("；")) }
            ));
        }

        // 规则 2：工具只对 General 有意义，其他角色的工具需求直接清空
        if route.execution_role != Role::General && !route.required_tools.is_empty() {
            notes.push(format!("{} 不使用内置工具，已忽略 required_tools", route.execution_role.as_str()));
            route.required_tools.clear();
        }

        // 规则 3：高风险任务需要人工审批
        let auto_execute = route.risk_level < self.approval_risk;
        if !auto_execute {
            notes.push(format!("风险等级 {:?} 需要人工审批", route.risk_level));
        }

        // 规则 4：哪些任务执行后需要独立审查
        //   - 代码任务一律审查
        //   - 需要生成文档的通用任务（L3/L4）审查
        //   - Reviewer 自己的任务不再审查
        let wants_review = match route.execution_role {
            Role::Coding => true,
            Role::General => matches!(route.task_class, TaskClass::L3 | TaskClass::L4),
            Role::Reviewer => false,
        };
        let needs_review = wants_review && registry.is_enabled(Role::Reviewer);
        if wants_review && !needs_review {
            notes.push("需要审查但 Reviewer 不可用，跳过审查".into());
        }

        Ok(PolicyVerdict { auto_execute, needs_review, notes })
    }
}
