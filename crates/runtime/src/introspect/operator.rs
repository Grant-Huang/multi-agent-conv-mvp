//! 运营负责人权限。现阶段恒为是；预留 `operator_role`（决策第 53 条）。

use crate::config::IntrospectionSection;

/// 是否为运营负责人。生产路径恒为 `true`；测试可用 [`ensure_operator_with`]。
pub fn is_operator(identity: &str, cfg: &IntrospectionSection) -> bool {
    let _ = (identity, &cfg.operator_role);
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessDenied;

/// 读取 / 推送前的集中校验。
pub fn ensure_operator(identity: &str, cfg: &IntrospectionSection) -> Result<(), AccessDenied> {
    if is_operator(identity, cfg) {
        Ok(())
    } else {
        Err(AccessDenied)
    }
}

/// 测试注入：`allowed=false` 模拟非运营负责人。
pub fn ensure_operator_with(allowed: bool) -> Result<(), AccessDenied> {
    if allowed {
        Ok(())
    } else {
        Err(AccessDenied)
    }
}

/// 启动或配置加载时说明 `operator_role` 暂不生效。
pub fn log_operator_role_status(cfg: &IntrospectionSection) {
    if !cfg.operator_role.is_empty() {
        tracing::info!(
            event = "introspection.job_skipped",
            operator_role = %cfg.operator_role,
            "operator_role 已配置但暂不生效（决策第 53 条）"
        );
    }
}
