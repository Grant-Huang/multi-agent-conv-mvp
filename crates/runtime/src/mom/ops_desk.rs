//! 运营频道发布器与事件分级（docs/mom/OPS_DESK.md，DECISIONS 第 55–57 条）。
//!
//! 实现前骨架：只有类型和签名，行为是故意不完整的，让验收测试先出现业务断言失败（Red）。

use super::{Engine, BoardEvent};
use crate::store::Store;
use anyhow::Result;
use std::{path::Path, sync::Arc};

/// 运营频道的固定会话 id
pub const OPS_CHANNEL_ID: &str = "ops-channel";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpsLevel {
    P0,
    P1,
    P2,
}

/// 事件分级：只看事件种类和案例当前阶段，不调用模型。
pub fn classify(_kind: &str, _stage: &str) -> OpsLevel {
    OpsLevel::P1
}

#[derive(Debug, Clone)]
pub struct OpsConfig {
    pub merge_secs: u64,
    pub hourly_cap: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub posted: u32,
    pub updated: u32,
    pub summarized: u32,
    pub skipped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipRecord {
    pub case_id: String,
    pub seq: u64,
    pub reason: String,
}

pub struct OpsPublisher {
    _store: Arc<Store>,
    _cfg: OpsConfig,
}

impl OpsPublisher {
    pub fn memory(store: Arc<Store>, cfg: OpsConfig) -> Result<Self> {
        Ok(Self { _store: store, _cfg: cfg })
    }
    pub fn open(_path: &Path, store: Arc<Store>, cfg: OpsConfig) -> Result<Self> {
        Ok(Self { _store: store, _cfg: cfg })
    }
    /// 消费引擎发件箱里的新事件，把需要的案例发到运营频道或更新原卡片。`now_secs` 由调用方给，保证可重放。
    pub fn tick(&mut self, _engine: &mut Engine, _now_secs: u64) -> Result<TickReport> {
        let _: Option<BoardEvent> = None;
        Ok(TickReport::default())
    }
    /// 每次“没发”的判断及原因
    pub fn skip_log(&self) -> Result<Vec<SkipRecord>> {
        Ok(vec![])
    }
}
