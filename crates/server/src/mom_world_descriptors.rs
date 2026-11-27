//! World Adapter 连接器描述符与健康探测（连接器契约 K4/K6，docs/contracts/connector.md §5/§6④；Issue #488）。
//!
//! 三个世界后端（mock / des / prod）各给一条连接器描述符与一个健康探测端点：
//! 描述符以代码常量暴露（契约 §7 第 3 条「描述符载体」尚未拍板，connectors.toml
//! 配置文件形态待拍板后再迁）；健康探测按 K4——明确返回可用形态，不可用不得无声失败
//! （mock / des 为进程内适配恒可用，prod 为 M3 桩数据模式、真实站点未接）。
//!
//! 超时、取消与清理：纯只读端点，无子进程、无后台任务。

use axum::{extract::Path, routing::get, Json, Router};
use serde_json::{json, Value};

/// 描述符字段对齐契约 K6 最小字段（id/kind/transport/endpoint/credentials 引用/
/// capabilities/health/owner/docs_url；timeout/retry 进程内适配不适用，留 null）。
#[derive(Debug, Clone)]
struct ConnectorDescriptor {
    id: &'static str,
    kind: &'static str,
    transport: &'static str,
    endpoint: &'static str,
    /// K2 凭据引用；三个世界后端当前均不持有外部凭据（prod 真实站点接入时改为 env 引用）
    credentials: Option<&'static str>,
    capabilities: &'static [&'static str],
    probe: &'static str,
    interval_secs: u64,
    owner: &'static str,
}

const MOCK: ConnectorDescriptor = ConnectorDescriptor {
    id: "world.mock",
    kind: "business",
    transport: "http",
    endpoint: "/api/world/mock",
    credentials: None,
    capabilities: &[
        "observe",
        "propose",
        "execute",
        "experiment",
        "ops_projection",
    ],
    probe: "GET /api/world/mock/health",
    interval_secs: 300,
    owner: "line3",
};
const DES: ConnectorDescriptor = ConnectorDescriptor {
    id: "world.des",
    kind: "business",
    transport: "http",
    endpoint: "/api/world/des",
    credentials: None,
    capabilities: &[
        "observe",
        "propose",
        "execute",
        "confirm",
        "experiment",
        "ground_truth",
    ],
    probe: "GET /api/world/des/health",
    interval_secs: 300,
    owner: "line3",
};
const PROD: ConnectorDescriptor = ConnectorDescriptor {
    id: "world.prod",
    kind: "business",
    transport: "http",
    endpoint: "/api/world/prod",
    credentials: None,
    capabilities: &["observe", "propose", "execute_stub", "bind"],
    probe: "GET /api/world/prod/health",
    interval_secs: 300,
    owner: "line3",
};

fn descriptor_json(d: &ConnectorDescriptor) -> Value {
    json!({
        "id": d.id,
        "kind": d.kind,
        "transport": d.transport,
        "endpoint": d.endpoint,
        "credentials": d.credentials,
        "capabilities": d.capabilities,
        "health": {"probe": d.probe, "interval_secs": d.interval_secs},
        "timeout_secs": null,
        "retry": null,
        "owner": d.owner,
        "docs_url": "docs/contracts/connector.md",
    })
}

fn all_descriptors() -> Vec<Value> {
    [&MOCK, &DES, &PROD]
        .into_iter()
        .map(descriptor_json)
        .collect()
}

async fn descriptors() -> Json<Value> {
    Json(json!({"descriptors": all_descriptors()}))
}

/// K4 健康探测：明确返回可用形态。mock / des 进程内恒可用；prod 为 M3 桩数据模式
/// （真实站点未接，见 WA_M3_TASKS），降级声明为 stub——不得无声失败。
async fn health(
    Path(backend): Path<String>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    let (descriptor, status, mode, detail) = match backend.as_str() {
        "mock" => (&MOCK, "ok", "in_process", "内存桩，随服务进程可用"),
        "des" => (
            &DES,
            "ok",
            "in_process",
            "内存桩或远程 DES 连接器（随配置）",
        ),
        "prod" => (
            &PROD,
            "ok",
            "stub",
            "M3 桩数据模式：只读接口以桩数据跑通，真实站点未接（写入未开工）",
        ),
        other => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({"error": "E_UNKNOWN_BACKEND", "backend": other})),
            ))
        }
    };
    Ok(Json(json!({
        "id": descriptor.id,
        "status": status,
        "mode": mode,
        "detail": detail,
        "probe": descriptor.probe,
        "checked_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })))
}

pub fn router() -> Router {
    Router::new()
        .route("/api/world/descriptors", get(descriptors))
        .route("/api/world/{backend}/health", get(health))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn descriptors_carry_k6_minimum_fields() {
        let Json(body) = descriptors().await;
        let list = body["descriptors"].as_array().unwrap();
        assert_eq!(list.len(), 3, "mock / des / prod 三个后端描述符");
        for d in list {
            for field in [
                "id",
                "kind",
                "transport",
                "endpoint",
                "capabilities",
                "health",
                "owner",
            ] {
                assert!(
                    !d[field].is_null(),
                    "{id} 缺 K6 最小字段 {field}",
                    id = d["id"]
                );
            }
            assert_eq!(d["kind"], "business");
            assert_eq!(
                d["credentials"],
                Value::Null,
                "当前均不持有外部凭据（K2 引用形态在真实站点接入时引入）"
            );
        }
        let ids: Vec<&str> = list.iter().map(|d| d["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["world.mock", "world.des", "world.prod"]);
    }

    #[tokio::test]
    async fn health_reports_mode_and_unknown_backend_is_404() {
        for (backend, mode) in [
            ("mock", "in_process"),
            ("des", "in_process"),
            ("prod", "stub"),
        ] {
            let Ok(Json(body)) = health(Path(backend.to_string())).await else {
                panic!("{backend} 应健康");
            };
            assert_eq!(body["status"], "ok", "{backend}");
            assert_eq!(body["mode"], mode, "{backend}");
        }
        // K4：未知后端明确 404，不得无声失败
        let Err((status, _)) = health(Path("no-such".to_string())).await else {
            panic!("未知后端应 404");
        };
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
