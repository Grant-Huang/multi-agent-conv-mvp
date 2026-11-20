//! 运营负责人权限。现阶段恒为是；预留 `operator_role`（决策第 53 条）。

use std::sync::atomic::{AtomicI8, Ordering};

use crate::config::IntrospectionSection;

/// `-1` = 默认生产行为；`0` = 拒绝；`1` = 允许。供 HTTP 集成测试跨线程注入。
static OPERATOR_OVERRIDE: AtomicI8 = AtomicI8::new(-1);
static OPERATOR_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 占用测试锁后再改 override，避免并行用例互相踩。
pub async fn lock_operator_override() -> tokio::sync::MutexGuard<'static, ()> {
    OPERATOR_TEST_LOCK.lock().await
}

/// 测试注入：`Some(false)` 模拟非运营；`None` 恢复默认。须持有 [`lock_operator_override`]。
pub fn set_operator_override(allowed: Option<bool>) {
    let v = match allowed {
        None => -1,
        Some(true) => 1,
        Some(false) => 0,
    };
    OPERATOR_OVERRIDE.store(v, Ordering::SeqCst);
}

/// 是否为运营负责人。生产路径恒为 `true`；测试可用 [`set_operator_override`]。
pub fn is_operator(identity: &str, cfg: &IntrospectionSection) -> bool {
    let _ = (identity, &cfg.operator_role);
    match OPERATOR_OVERRIDE.load(Ordering::SeqCst) {
        0 => false,
        1 => true,
        _ => true,
    }
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
