//! 业务内省：定时分析运营数据，经发送闸门后才推送给运营负责人。
//!
//! 默认关闭（`[introspection] enabled = false`）。方案与进度见
//! `docs/memory/INTROSPECTION_PLAN.md`、`docs/PLAN.md` 的 INT 一节。

pub mod model;
