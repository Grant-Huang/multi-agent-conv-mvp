//! 排查第 3、4 层（AG.5）：诊断包、请其他助手帮忙（每次预览、逐次同意，经 DLP）。
//! 实现前的占位：测试用的两个方法先给空实现，让用例能编译并跑出业务断言失败。

use std::time::Duration;

use crate::Runtime;

impl Runtime {
    /// 测试用：让所有未使用的同意凭证立即过期
    #[doc(hidden)]
    pub fn expire_diagnose_consents(&self) {}

    /// 测试用：调整帮手单次运行的时限（默认 120 秒）
    #[doc(hidden)]
    pub fn set_diagnose_helper_timeout(&self, _limit: Duration) {}
}
