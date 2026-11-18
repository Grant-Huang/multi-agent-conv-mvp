//! 可复现的随机数流（设计文档 §4.5）。
//!
//! 每个随机源（每台设备的磨损、振动噪声、每条产线的订单与来料批次……）各用一条独立的流，
//! 由 `seed + 尝试序号 + 用途标签` 派生。增加一台设备或一个随机源，不会改变其他流的序列。
//! 只用整数运算和标准库浮点函数，同一平台上结果逐位一致。

/// SplitMix64 混合函数（与 `simulation.rs` 中脚本队列使用的同一族算法）。
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// FNV-1a 字符串哈希，用于把用途标签变成流编号。
fn fnv(label: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in label.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub struct Rng {
    state: u64,
}

impl Rng {
    /// 派生一条独立的随机流。
    pub fn stream(seed: u64, attempt: u32, label: &str) -> Self {
        let state = mix(seed ^ mix(fnv(label) ^ mix(u64::from(attempt))));
        Self { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut x = self.state;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 31)
    }

    /// (0, 1) 开区间均匀分布，避免取对数时出现 0。
    pub fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    pub fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// 标准正态分布（Box-Muller）。
    pub fn normal(&mut self) -> f64 {
        let (u1, u2) = (self.unit(), self.unit());
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }

    /// 均值为 `mean` 的指数分布（泊松过程的到达间隔）。
    pub fn exponential(&mut self, mean: f64) -> f64 {
        -mean * self.unit().ln()
    }

    /// 形状参数为 `shape`、尺度为 1 的 Gamma 分布（Marsaglia–Tsang；shape < 1 时用提升法）。
    pub fn gamma(&mut self, shape: f64) -> f64 {
        if shape < 1.0 {
            let boost = self.unit().powf(1.0 / shape);
            return self.gamma(shape + 1.0) * boost;
        }
        let d = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        loop {
            let x = self.normal();
            let v = (1.0 + c * x).powi(3);
            if v <= 0.0 {
                continue;
            }
            let u = self.unit();
            if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
                return d * v;
            }
        }
    }
}
