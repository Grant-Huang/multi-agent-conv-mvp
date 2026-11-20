//! 维修班组排班（审查修复 R-20261119-M13 / DES-30）。
//!
//! 模拟工厂与进程内仿真共用：开始时刻 = max(请求时刻, 最早空闲班组时刻)；
//! 班组占用累计此前所有已接受的维修。

use anyhow::{ensure, Context, Result};

/// 创建仿真时允许的最大起点（约 10 年，单位：虚拟分钟）。
pub const MAX_START_MINUTE: u64 = 5_256_000;

/// 场景时长与各类耗时上限（分钟）。
pub const MAX_DURATION_MINUTES: u64 = 100_000;

/// 按已接受的维修区间重建各班组的空闲时刻。
///
/// `prior` 为 `(start_minute, end_minute)`，按开始时刻排序后依次分给最早空闲的班组。
pub fn crew_free_after(crew_count: usize, prior: &[(u64, u64)]) -> Vec<u64> {
    let mut crews = vec![0u64; crew_count.max(1)];
    let mut jobs: Vec<(u64, u64)> = prior.to_vec();
    jobs.sort_by_key(|(start, _)| *start);
    for &(start, end) in &jobs {
        let i = (0..crews.len()).min_by_key(|&i| crews[i]).unwrap_or(0);
        let begin = crews[i].max(start);
        let duration = end.saturating_sub(start);
        crews[i] = begin.saturating_add(duration);
    }
    crews
}

/// 给下一张维修单选班组：开始 = max(请求时刻, 该班组空闲)，并更新占用。
pub fn schedule_next_maintenance(
    request_minute: u64,
    maintenance_minutes: u64,
    crew_free_at: &mut [u64],
) -> Result<(u64, u64)> {
    ensure!(!crew_free_at.is_empty(), "at least one maintenance crew");
    ensure!(
        (1..=MAX_DURATION_MINUTES).contains(&maintenance_minutes),
        "maintenance_minutes must be 1..={MAX_DURATION_MINUTES}"
    );
    let i = (0..crew_free_at.len())
        .min_by_key(|&i| crew_free_at[i])
        .unwrap_or(0);
    let begin = crew_free_at[i].max(request_minute);
    let end = begin
        .checked_add(maintenance_minutes)
        .context("maintenance end overflow")?;
    crew_free_at[i] = end;
    Ok((begin, end))
}

/// 校验工厂资源里的耗时字段不超过上限（创建仿真时调用）。
pub fn validate_resource_durations(
    maintenance_minutes: u64,
    tool_transfer_minutes: u64,
    finish_batch_minutes: u64,
    inspection_minutes: u64,
) -> Result<()> {
    for (name, value) in [
        ("maintenance_minutes", maintenance_minutes),
        ("tool_transfer_minutes", tool_transfer_minutes),
        ("finish_batch_minutes", finish_batch_minutes),
        ("inspection_minutes", inspection_minutes),
    ] {
        ensure!(
            value <= MAX_DURATION_MINUTES,
            "{name} must be at most {MAX_DURATION_MINUTES} minutes"
        );
    }
    Ok(())
}
