//! 规律模型（设计文档 §4.2）。所有常数都是**建模假设**，用于产生"有规律"的扰动，
//! 不代表真实设备或工艺能力；以后用工厂数据标定（台账 M27）。

use super::Mechanisms;

// ── 主轴磨损：Gamma 过程（只增不减，增量随机） ──
/// 正常负荷下每分钟平均磨损增量。约 12 小时增加 0.43。
pub(super) const WEAR_RATE_PER_MIN: f64 = 0.0006;
/// 每分钟增量的 Gamma 形状参数（越小越"跳跃"）。
pub(super) const WEAR_SHAPE_PER_MIN: f64 = 0.5;
/// 产线有插单积压时的负荷系数（插单 → 负荷 → 磨损加快）。
pub(super) const OVERLOAD_FACTOR: f64 = 1.4;
/// 功能失效阈值 F：达到后设备停机（P-F 曲线的 F 点）。
pub(super) const FUNCTIONAL_FAILURE_WEAR: f64 = 2.6;

// ── 振动：随磨损上升，按 ISO 20816-3 第 2 组刚性安装分区判断（边界值待对照标准原文） ──
const VIBRATION_BASE_MM_S: f64 = 0.9;
const VIBRATION_PER_WEAR: f64 = 1.6;
/// 振动测量噪声标准差（mm/s）。
pub(super) const VIBRATION_NOISE_MM_S: f64 = 0.08;
/// B/C 区边界：进入 C 区并持续若干分钟才报警。
pub(super) const VIBRATION_ZONE_C_MM_S: f64 = 2.8;
/// A/B 区边界：振动回到此值以下才解除报警锁存。
pub(super) const VIBRATION_REARM_MM_S: f64 = 1.4;
/// 持续越限的分钟数。
pub(super) const VIBRATION_SUSTAIN_MINUTES: u32 = 3;

// ── 尺寸与过程能力 ──
/// 单边公差（mm）：名义值 ±0.020。
const TOLERANCE_MM: f64 = 0.020;
/// 正常状态的过程标准差（mm），对应 Cpk ≈ 1.67。
const SIGMA0_MM: f64 = 0.004;
/// 主轴磨损开始影响尺寸的阈值（P 点附近）。
const SPINDLE_ONSET_WEAR: f64 = 1.2;
/// 超过阈值后每单位磨损造成的均值偏移（mm）。
const SPINDLE_SHIFT_PER_WEAR: f64 = 0.01;
/// 超过阈值后每单位磨损造成的标准差相对增幅。
const SPINDLE_SIGMA_PER_WEAR: f64 = 0.3;
/// 刀具名义寿命（件）。每把刀的实际寿命围绕名义值对数正态离散。
pub(super) const TOOL_NOMINAL_LIFE_PARTS: f64 = 300.0;
/// 实际寿命的对数标准差：约 11% 的刀会在计划换刀前进入急剧磨损段。
pub(super) const TOOL_LIFE_LOG_SIGMA: f64 = 0.15;
/// 按名义寿命的这个比例计划换刀（预防性换刀）。
pub(super) const TOOL_PLANNED_CHANGE_FRACTION: f64 = 0.75;
/// 仿真起点刀具最多已用到实际寿命的这个比例，保证起点处于受控状态。
pub(super) const TOOL_START_MAX_LIFE_FRACTION: f64 = 0.85;
/// 后刀面磨损超过参考值后造成的均值偏移系数。
const TOOL_SHIFT_PER_VB: f64 = 0.03;
const TOOL_VB_REFERENCE_MM: f64 = 0.10;
/// 某个机制的偏移量至少达到这个值，才计入质量报警的根因（mm）。
pub(super) const CAUSE_MIN_SHIFT_MM: f64 = 0.001;
/// Cpk 报警解除锁存：回到门槛以上这么多，并保持若干分钟。
pub(super) const CPK_REARM_MARGIN: f64 = 0.05;
pub(super) const CPK_REARM_MINUTES: u32 = 30;

/// 振动有效值的期望（mm/s），不含噪声。
pub(super) fn vibration_mean(wear: f64) -> f64 {
    VIBRATION_BASE_MM_S + VIBRATION_PER_WEAR * wear
}

/// 刀具后刀面磨损 VB（mm），按寿命比例分三段：磨合、稳定、急剧磨损。
pub(super) fn tool_flank_wear(life_fraction: f64) -> f64 {
    let f = life_fraction.clamp(0.0, 1.0);
    let break_in = 0.05 * (f / 0.1).min(1.0);
    let steady = 0.10 * f;
    let accelerated = if f > 0.8 {
        0.15 * ((f - 0.8) / 0.2).powi(2)
    } else {
        0.0
    };
    0.05 + break_in + steady + accelerated
}

/// 各机制对尺寸分布的贡献。机制关闭时对应贡献为 0。
#[derive(Clone, Copy, Debug)]
pub(super) struct Contributions {
    pub spindle_shift: f64,
    pub tool_shift: f64,
    pub lot_shift: f64,
    pub sigma: f64,
}

pub(super) fn contributions(
    m: &Mechanisms,
    spindle_wear: f64,
    tool_life_fraction: f64,
    lot_offset: f64,
) -> Contributions {
    let excess = (spindle_wear - SPINDLE_ONSET_WEAR).max(0.0);
    let (spindle_shift, sigma) = if m.spindle_wear_to_dimension {
        (
            SPINDLE_SHIFT_PER_WEAR * excess,
            SIGMA0_MM * (1.0 + SPINDLE_SIGMA_PER_WEAR * excess),
        )
    } else {
        (0.0, SIGMA0_MM)
    };
    let tool_shift = if m.tool_wear_to_dimension {
        TOOL_SHIFT_PER_VB * (tool_flank_wear(tool_life_fraction) - TOOL_VB_REFERENCE_MM).max(0.0)
    } else {
        0.0
    };
    let lot_shift = if m.material_lot_to_dimension {
        lot_offset
    } else {
        0.0
    };
    Contributions {
        spindle_shift,
        tool_shift,
        lot_shift,
        sigma,
    }
}

/// 过程能力（模型值）：Cpk = (公差 − |均值偏移|) / 3σ。
///
/// 阶段①使用由过程参数直接算出的模型值，不做抽样估计，因此没有估计噪声；
/// 基于测量样本的估计和 SPC 判异规则放到后续阶段（设计文档 §4.2 修订说明）。
pub(super) fn cpk(c: &Contributions) -> f64 {
    let shift = c.spindle_shift + c.tool_shift + c.lot_shift;
    (TOLERANCE_MM - shift.abs()) / (3.0 * c.sigma)
}
