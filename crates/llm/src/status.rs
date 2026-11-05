//! llama-server 的 `/slots` 和 `/metrics` 摘要，给诊断页用。

use serde::Serialize;
use serde_json::Value;

/// 一个槽位在服务端的状态。这个版本的 llama.cpp 不提供"已生成 token 数"，`tasks` 是该槽位处理过的任务数（`id_task`）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerSlot {
    pub id: u32,
    pub busy: bool,
    pub tasks: u64,
}

/// `/metrics` 里诊断需要的三项
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerMetrics {
    /// 正在生成的请求数
    pub processing: u64,
    /// 排队等待的请求数（`requests_deferred`）
    pub queued: u64,
    /// 平均生成速度（token/s）
    pub gen_per_sec: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ServerView {
    pub slots: Vec<ServerSlot>,
    pub metrics: ServerMetrics,
}

pub fn parse_slots(v: &Value) -> Option<Vec<ServerSlot>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|s| {
                Some(ServerSlot { id: s.get("id")?.as_u64()? as u32, busy: s["is_processing"].as_bool().unwrap_or(false), tasks: s["id_task"].as_u64().unwrap_or(0) })
            })
            .collect(),
    )
}

/// 从 Prometheus 文本里取一个指标。名字按冒号后的部分匹配（`llamacpp:requests_processing`）
pub fn metric(text: &str, name: &str) -> Option<f64> {
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut parts = line.split_whitespace();
        let key = parts.next()?;
        let short = key.rsplit(':').next().unwrap_or(key);
        if short == name { parts.next()?.parse().ok() } else { None }
    })
}

pub fn parse_metrics(text: &str) -> ServerMetrics {
    ServerMetrics {
        processing: metric(text, "requests_processing").unwrap_or(0.0) as u64,
        queued: metric(text, "requests_deferred").unwrap_or(0.0) as u64,
        gen_per_sec: metric(text, "predicted_tokens_seconds").unwrap_or(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slots_report_busy_flag_and_task_count() {
        let v = json!([
            {"id": 0, "is_processing": false, "id_task": 4},
            {"id": 2, "is_processing": true, "id_task": 7}
        ]);
        let s = parse_slots(&v).unwrap();
        assert_eq!(s, vec![ServerSlot { id: 0, busy: false, tasks: 4 }, ServerSlot { id: 2, busy: true, tasks: 7 }]);
        assert!(parse_slots(&json!({})).is_none());
    }

    #[test]
    fn metrics_pick_the_three_gauges_and_ignore_comments() {
        let text = "\
# HELP llamacpp:predicted_tokens_seconds Average generation throughput in tokens/s
llamacpp:predicted_tokens_seconds 27.8819
# HELP llamacpp:requests_processing Number of requests processing
llamacpp:requests_processing 2
llamacpp:requests_deferred 5
llamacpp:prompt_tokens_total 4461
";
        let m = parse_metrics(text);
        assert_eq!(m.processing, 2);
        assert_eq!(m.queued, 5);
        assert!((m.gen_per_sec - 27.8819).abs() < 1e-6, "{}", m.gen_per_sec);
        // 缺字段时按 0，不报错
        assert_eq!(parse_metrics("").processing, 0);
    }
}
