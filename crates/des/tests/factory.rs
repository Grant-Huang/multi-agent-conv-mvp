//! 阶段⑤a-1：模拟工厂服务（`an_des::FactoryService`）的工具契约。
//!
//! 用例见 `docs/testing/CASES.md`「模拟工厂与 MCP 挂载（阶段⑤a-1）」。所有调用都经 `call(工具名, JSON)`，
//! 与 MCP 的 `tools/call` 一一对应；进程内与 MCP 两种模式的一致性见 RT-DESM-02 与 RT-DES-30。

use an_des::{tool_catalog, FactoryService, ServiceConfig, ToolCategory};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// S1 夹具的起点：08:17 = 第 497 分钟。
const START: u64 = 497;
const ADMIN: &str = "test-admin-token";

fn service() -> FactoryService {
    FactoryService::new(ServiceConfig {
        admin_token: Some(ADMIN.into()),
    })
}

fn create(s: &mut FactoryService, seed: u64, pattern: Value, horizon: u64) -> String {
    let r = s
        .call(
            "simulation_create",
            json!({
                "name": format!("t-{seed}"),
                "factory": null,
                "scenario": {"seed": seed, "pattern": pattern, "start_minute": START, "horizon_minutes": horizon, "mechanisms": null}
            }),
        )
        .expect("simulation_create");
    assert_eq!(
        r["now"], START,
        "a new simulation starts at its start minute"
    );
    r["simulation_id"]
        .as_str()
        .expect("simulation id")
        .to_string()
}

fn p0(s: &mut FactoryService) -> String {
    create(s, 1, json!({"kind": "script"}), 240)
}

fn advance(s: &mut FactoryService, sim: &str, to: u64) -> Value {
    s.call(
        "simulation_advance",
        json!({"simulation_id": sim, "to_minute": to}),
    )
    .expect("simulation_advance")
}

fn poll(s: &mut FactoryService, sim: &str, cursor: u64, limit: u64) -> Value {
    s.call(
        "events_poll",
        json!({"simulation_id": sim, "cursor": cursor, "limit": limit}),
    )
    .expect("events_poll")
}

/// 从游标 0 拉取全部已发生事件。
fn all_events(s: &mut FactoryService, sim: &str) -> Vec<Value> {
    let mut out = vec![];
    let mut cursor = 0;
    loop {
        let page = poll(s, sim, cursor, 1000);
        let events = page["events"].as_array().expect("events").clone();
        let next = page["next_cursor"].as_u64().expect("next cursor");
        if events.is_empty() {
            assert_eq!(next, cursor, "an empty page keeps the cursor");
            return out;
        }
        out.extend(events);
        cursor = next;
    }
}

fn machine(status: &Value, id: &str) -> Value {
    status["machines"]
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["id"] == id)
        .unwrap_or_else(|| panic!("machine {id} missing: {status}"))
        .clone()
}

fn status(s: &mut FactoryService, sim: &str) -> Value {
    s.call("equipment_status", json!({"simulation_id": sim}))
        .expect("equipment_status")
}

/// 所有观测工具的输出（用于比较"读取不改变世界"）。
fn observe_all(s: &mut FactoryService, sim: &str) -> Value {
    json!({
        "status": status(s, sim),
        "trend": s.call("equipment_trend", json!({"simulation_id": sim})).expect("equipment_trend"),
        "quality": s.call("quality_capability", json!({"simulation_id": sim})).expect("quality_capability"),
        "tooling": s.call("inventory_tooling", json!({"simulation_id": sim})).expect("inventory_tooling"),
        "events": all_events(s, sim),
    })
}

fn write_args(sim: &str, extra: Value, key: &str) -> Value {
    let mut args = json!({"simulation_id": sim, "idempotency_key": key, "approval_ref": "CASE-s1-1:rev1:production"});
    for (k, v) in extra.as_object().unwrap() {
        args[k] = v.clone();
    }
    args
}

#[test]
fn rt_desf_01_tool_catalog_is_generic_and_categorised() {
    let catalog = tool_catalog();
    let by_name: BTreeMap<&str, ToolCategory> = catalog
        .iter()
        .map(|t| (t.name.as_str(), t.category))
        .collect();
    let expected = [
        ("equipment_status", ToolCategory::Observe),
        ("equipment_trend", ToolCategory::Observe),
        ("quality_capability", ToolCategory::Observe),
        ("inventory_tooling", ToolCategory::Observe),
        ("events_poll", ToolCategory::Observe),
        ("maintenance_order_create", ToolCategory::Execute),
        ("tooling_transfer_create", ToolCategory::Execute),
        ("production_schedule_apply", ToolCategory::Execute),
        ("order_status", ToolCategory::Observe),
        ("simulation_create", ToolCategory::Control),
        ("simulation_advance", ToolCategory::Control),
        ("simulation_fault_inject", ToolCategory::Control),
        ("simulation_ground_truth", ToolCategory::Admin),
    ];
    assert_eq!(
        by_name.len(),
        catalog.len(),
        "tool names must be unique: {catalog:?}"
    );
    assert_eq!(
        by_name,
        expected.into_iter().collect::<BTreeMap<_, _>>(),
        "catalog must match the design table"
    );
    for t in &catalog {
        // Anthropic / OpenAI 工具名规则的交集，且不含大写与连字符，便于其他系统挂载。
        assert!(
            !t.name.is_empty()
                && t.name.len() <= 64
                && t.name.starts_with(|c: char| c.is_ascii_lowercase())
                && t.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "tool name {} must match ^[a-z][a-z0-9_]{{0,63}}$",
            t.name
        );
        assert!(
            !t.description.trim().is_empty(),
            "{} needs a description",
            t.name
        );
        assert_eq!(t.input_schema["type"], "object", "{} schema", t.name);
        assert_eq!(
            t.input_schema["additionalProperties"], false,
            "{} must reject unknown fields",
            t.name
        );
        for word in ["agentnexus", "case", "mom"] {
            assert!(
                !t.name.contains(word),
                "{} must not use AgentNexus-specific concepts",
                t.name
            );
        }
        if t.category == ToolCategory::Execute {
            let required = t.input_schema["required"].as_array().expect("required");
            for f in ["idempotency_key", "approval_ref"] {
                assert!(
                    required.iter().any(|r| r == f),
                    "{} must require {f}",
                    t.name
                );
            }
        }
    }
}

#[test]
fn rt_desf_02_same_seed_same_results_and_cursor_paging() {
    let run = || {
        let mut s = service();
        let sim = create(
            &mut s,
            7,
            json!({"kind": "concurrent_cluster", "window_minutes": 60, "min_disruptions": 3}),
            240,
        );
        advance(&mut s, &sim, START + 240);
        (all_events(&mut s, &sim), status(&mut s, &sim))
    };
    let (a, sa) = run();
    let (b, sb) = run();
    assert!(!a.is_empty(), "the cluster pattern produces events");
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap(),
        "same inputs, byte-identical events"
    );
    assert_eq!(sa, sb, "same inputs, same observable state");

    // 游标：分页拼接等于一次拉取；只返回已发生的事件；同一游标重复拉取结果相同。
    let mut s = service();
    let sim = create(
        &mut s,
        7,
        json!({"kind": "concurrent_cluster", "window_minutes": 60, "min_disruptions": 3}),
        240,
    );
    let mid = START + 120;
    advance(&mut s, &sim, mid);
    let mut paged = vec![];
    let mut cursor = 0;
    loop {
        let page = poll(&mut s, &sim, cursor, 2);
        let events = page["events"].as_array().unwrap().clone();
        assert!(events.len() <= 2, "limit is honoured");
        assert_eq!(page["now"], mid);
        if events.is_empty() {
            break;
        }
        assert_eq!(page, poll(&mut s, &sim, cursor, 2), "polling is repeatable");
        paged.extend(events);
        cursor = page["next_cursor"].as_u64().unwrap();
    }
    assert!(
        paged
            .iter()
            .all(|e| e["occurred_at"].as_u64().unwrap() <= mid),
        "no future events"
    );
    let ids: Vec<&Value> = paged.iter().map(|e| &e["id"]).collect();
    let mut sorted = ids.clone();
    sorted.dedup();
    assert_eq!(ids.len(), sorted.len(), "no duplicates across pages");
    assert!(
        paged
            .iter()
            .all(|e| e["id"].as_str().unwrap().starts_with(&format!("{sim}:"))),
        "event ids carry the simulation id"
    );
    advance(&mut s, &sim, START + 240);
    let full = all_events(&mut s, &sim);
    assert_eq!(
        &full[..paged.len()],
        &paged[..],
        "delivered events never change"
    );
}

#[test]
fn rt_desf_03_observation_tools_are_read_only() {
    // 两个服务：一个在推进过程中反复读取，一个不读；最终世界必须相同。
    let mut reader = service();
    let mut quiet = service();
    let sr = p0(&mut reader);
    let sq = p0(&mut quiet);
    for t in [START + 5, START + 20, START + 60] {
        advance(&mut reader, &sr, t);
        let first = observe_all(&mut reader, &sr);
        assert_eq!(first, observe_all(&mut reader, &sr), "reads are repeatable");
        advance(&mut quiet, &sq, t);
    }
    advance(&mut reader, &sr, START + 240);
    advance(&mut quiet, &sq, START + 240);
    assert_eq!(
        observe_all(&mut reader, &sr),
        observe_all(&mut quiet, &sq),
        "reading must not change the simulated world"
    );
    let st = status(&mut reader, &sr);
    assert_eq!(st["provenance"], "simulation");
    let m = machine(&st, "CNC-XX");
    for k in ["vibration_mm_s", "zone", "cpk", "status", "tool_present"] {
        assert!(m.get(k).is_some(), "observable field {k}: {m}");
    }
}

#[test]
fn rt_desf_04_write_tools_need_approval_and_are_idempotent() {
    let mut s = service();
    let sim = p0(&mut s);
    advance(&mut s, &sim, START + 16);
    let before = machine(&status(&mut s, &sim), "CNC-XX");
    assert!(
        matches!(before["zone"].as_str(), Some("C" | "D")),
        "P0 machine is alarmed before maintenance: {before}"
    );
    let order = json!({"machine": "CNC-XX", "start_minute": START + 16});

    // 缺审批引用或幂等键：拒绝，且不改变世界。
    for missing in ["approval_ref", "idempotency_key"] {
        let mut args = write_args(&sim, order.clone(), "k1");
        args.as_object_mut().unwrap().remove(missing);
        assert!(
            s.call("maintenance_order_create", args).is_err(),
            "missing {missing} must be rejected"
        );
    }
    let mut blank = write_args(&sim, order.clone(), "k1");
    blank["approval_ref"] = json!("  ");
    assert!(
        s.call("maintenance_order_create", blank).is_err(),
        "blank approval_ref"
    );

    let receipt = s
        .call(
            "maintenance_order_create",
            write_args(&sim, order.clone(), "k1"),
        )
        .expect("maintenance order accepted");
    assert_eq!(receipt["status"], "accepted");
    let order_id = receipt["order_id"].as_str().expect("order id").to_string();
    let done = receipt["complete_minute"]
        .as_u64()
        .expect("complete minute");
    assert!(done > START + 16, "maintenance takes time");

    // 同键同参数：同一回执，只生效一次；同键不同参数：拒绝。
    let again = s
        .call(
            "maintenance_order_create",
            write_args(&sim, order.clone(), "k1"),
        )
        .expect("retry with the same key");
    assert_eq!(again, receipt, "a retry returns the original receipt");
    let other = json!({"machine": "CNC-XY", "start_minute": START + 16});
    assert!(
        s.call("maintenance_order_create", write_args(&sim, other, "k1"))
            .is_err(),
        "same key with different arguments must be rejected"
    );

    let st = |s: &mut FactoryService| {
        s.call(
            "order_status",
            json!({"simulation_id": sim, "order_id": order_id}),
        )
        .expect("order_status")["status"]
            .clone()
    };
    assert_eq!(st(&mut s), "accepted");
    advance(&mut s, &sim, done + 5);
    assert_eq!(st(&mut s), "completed");
    let after = machine(&status(&mut s, &sim), "CNC-XX");
    assert!(
        after["vibration_mm_s"].as_f64().unwrap() < 2.8,
        "maintenance restores the machine: {after}"
    );

    // 只下一次单：与另起一个服务只下一次单的世界相同。
    let mut once = service();
    let sim2 = p0(&mut once);
    advance(&mut once, &sim2, START + 16);
    once.call(
        "maintenance_order_create",
        write_args(&sim2, order, "other-key"),
    )
    .unwrap();
    advance(&mut once, &sim2, done + 5);
    assert_eq!(
        status(&mut s, &sim)["machines"],
        status(&mut once, &sim2)["machines"],
        "a retried order takes effect once"
    );

    let schedule = s
        .call(
            "production_schedule_apply",
            write_args(&sim, json!({"plan_id": "C", "order": "ORD-1"}), "k2"),
        )
        .expect("schedule applied");
    assert_eq!(schedule["status"], "accepted");
}

#[test]
fn rt_desf_05_tool_transfer_and_injected_fault() {
    let mut s = service();
    let sim = p0(&mut s);
    advance(&mut s, &sim, START + 16);
    let tooling = s
        .call("inventory_tooling", json!({"simulation_id": sim}))
        .unwrap();
    let spares = tooling["tool_spares"].as_u64().expect("tool spares");
    assert!(spares > 0, "{tooling}");
    let ok = s
        .call(
            "tooling_transfer_create",
            write_args(&sim, json!({"machine": "CNC-XZ"}), "t1"),
        )
        .expect("transfer accepted");
    assert_eq!(ok["status"], "accepted");
    assert_eq!(
        ok["complete_minute"].as_u64().unwrap(),
        START
            + 16
            + tooling["transfer_minutes"]
                .as_u64()
                .expect("transfer minutes")
    );

    s.call(
        "simulation_fault_inject",
        json!({"simulation_id": sim, "fault": {"kind": "tool_spares_lost"}}),
    )
    .expect("fault injected");
    let failed = s
        .call(
            "tooling_transfer_create",
            write_args(&sim, json!({"machine": "CNC-XY"}), "t2"),
        )
        .expect("a failed transfer still returns a receipt");
    assert_eq!(failed["status"], "failed");
    assert!(
        failed["reason"]
            .as_str()
            .is_some_and(|r| r.contains("spares")),
        "the reason comes from the simulation: {failed}"
    );
}

#[test]
fn rt_desf_06_ground_truth_needs_admin_token_and_never_leaks() {
    let mut s = service();
    let sim = p0(&mut s);
    advance(&mut s, &sim, START + 240);
    assert!(
        s.call("simulation_ground_truth", json!({"simulation_id": sim}))
            .is_err(),
        "no token"
    );
    assert!(
        s.call(
            "simulation_ground_truth",
            json!({"simulation_id": sim, "admin_token": "wrong"})
        )
        .is_err(),
        "wrong token"
    );
    let truth = s
        .call(
            "simulation_ground_truth",
            json!({"simulation_id": sim, "admin_token": ADMIN}),
        )
        .expect("admin reads the truth");
    assert!(
        truth["alarms"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x["root_causes"].is_array())),
        "{truth}"
    );
    let mut no_admin = FactoryService::new(ServiceConfig::default());
    let sim2 = p0(&mut no_admin);
    assert!(
        no_admin
            .call(
                "simulation_ground_truth",
                json!({"simulation_id": sim2, "admin_token": ADMIN})
            )
            .is_err(),
        "without a configured token the admin tool is closed"
    );

    // 其他工具的输出不含标准答案与隐状态。
    let text = observe_all(&mut s, &sim).to_string();
    for word in [
        "root_causes",
        "mechanisms",
        "spindle_wear",
        "relations",
        "contentions",
    ] {
        assert!(!text.contains(word), "observation leaks {word}");
    }
}

#[test]
fn rt_desf_07_invalid_requests_fail_fast_without_side_effects() {
    let mut s = service();
    let sim = p0(&mut s);
    advance(&mut s, &sim, START + 30);
    let snapshot = observe_all(&mut s, &sim);
    let bad = [
        ("no_such_tool", json!({})),
        ("equipment_status", json!({"simulation_id": "DES-999"})),
        (
            "equipment_status",
            json!({"simulation_id": sim, "extra": 1}),
        ),
        (
            "simulation_advance",
            json!({"simulation_id": sim, "to_minute": START}),
        ),
        (
            "simulation_advance",
            json!({"simulation_id": sim, "to_minute": START + 10_000_000}),
        ),
        (
            "events_poll",
            json!({"simulation_id": sim, "cursor": 0, "limit": 0}),
        ),
        (
            "events_poll",
            json!({"simulation_id": sim, "cursor": 0, "limit": 1001}),
        ),
        (
            "maintenance_order_create",
            write_args(
                &sim,
                json!({"machine": "CNC-NOPE", "start_minute": START + 30}),
                "x1",
            ),
        ),
        (
            "maintenance_order_create",
            write_args(
                &sim,
                json!({"machine": "CNC-XX", "start_minute": START}),
                "x2",
            ),
        ),
        (
            "order_status",
            json!({"simulation_id": sim, "order_id": "nope"}),
        ),
        (
            "simulation_fault_inject",
            json!({"simulation_id": sim, "fault": {"kind": "meteor"}}),
        ),
        (
            "simulation_create",
            json!({"name": "x", "factory": null, "scenario": {"seed": 1, "pattern": {"kind": "script"}, "start_minute": START, "horizon_minutes": 0, "mechanisms": null}}),
        ),
    ];
    for (tool, args) in bad {
        let started = std::time::Instant::now();
        assert!(
            s.call(tool, args.clone()).is_err(),
            "{tool} {args} must be rejected"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{tool} must fail fast"
        );
    }
    assert_eq!(
        observe_all(&mut s, &sim),
        snapshot,
        "rejected calls leave the world unchanged"
    );
}

#[test]
fn rt_desf_08_simulation_count_is_bounded_and_ids_are_sequential() {
    let mut s = service();
    let first = p0(&mut s);
    let second = p0(&mut s);
    assert_ne!(first, second);
    assert!(first.starts_with("DES-") && second.starts_with("DES-"));
    let mut created = 2;
    let err = loop {
        match s.call(
            "simulation_create",
            json!({"name": "n", "factory": null, "scenario": {"seed": created, "pattern": {"kind": "quiet"}, "start_minute": START, "horizon_minutes": 10, "mechanisms": null}}),
        ) {
            Ok(_) => created += 1,
            Err(e) => break e,
        }
        assert!(
            created <= 1000,
            "the service must cap the number of simulations"
        );
    };
    assert!(
        err.to_string().contains("capacity"),
        "a full service says so: {err}"
    );
}

/// 审查修复 R-20261119-M2：溢出的时刻与时长参数作为业务错误被拒绝，不 panic，服务和已有仿真继续可用。
#[test]
fn rt_desf_09_overflowing_minutes_are_rejected_without_panic() {
    let mut s = service();
    let sim = p0(&mut s);
    let before = observe_all(&mut s, &sim);
    let scenario = |start: u64, horizon: u64| json!({"seed": 1, "pattern": {"kind": "script"}, "start_minute": start, "horizon_minutes": horizon, "mechanisms": null});
    // 默认工厂只把维修耗时改成 u64::MAX（其余字段合法，确保被拒绝的原因是溢出）。
    let mut slow = serde_json::to_value(an_des::FactoryConfig::line_b_default()).unwrap();
    slow["resources"]["maintenance_minutes"] = json!(u64::MAX);
    let bad_creates = [
        json!({"name": "late", "factory": null, "scenario": scenario(u64::MAX - 10, 60)}),
        json!({"name": "long", "factory": null, "scenario": scenario(START, u64::MAX)}),
        json!({"name": "slow repair", "factory": slow, "scenario": scenario(START, 60)}),
    ];
    for args in bad_creates {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.call("simulation_create", args.clone())
        }));
        let result = outcome.unwrap_or_else(|_| panic!("simulation_create panicked on {args}"));
        assert!(
            result.is_err(),
            "overflowing input must be rejected: {args}"
        );
    }
    // 写操作：开始时刻接近上限时同样是业务错误。
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.call(
            "maintenance_order_create",
            write_args(
                &sim,
                json!({"machine": "CNC-XX", "start_minute": u64::MAX}),
                "overflow-1",
            ),
        )
    }));
    assert!(
        outcome
            .expect("maintenance_order_create must not panic")
            .is_err(),
        "a start minute near the limit is rejected"
    );
    // 被拒绝的调用不改变任何状态，已有仿真照常可用。
    assert_eq!(observe_all(&mut s, &sim), before, "no state change");
    let next = p0(&mut s);
    assert_eq!(next, "DES-2", "rejected creates do not take ids");
}

/// 审查修复 R-20261119-M13：维修单按维修班组排队（与进程内仿真同一规则），同一设备的重叠维修单被拒绝。
#[test]
fn rt_desf_10_maintenance_orders_queue_for_crews() {
    let mut s = service();
    let sim = p0(&mut s);
    let minutes = an_des::ResourceConfig::default().maintenance_minutes;
    assert_eq!(an_des::ResourceConfig::default().maintenance_crews, 1);
    let order = |s: &mut FactoryService, machine: &str, key: &str| {
        s.call(
            "maintenance_order_create",
            write_args(
                &sim,
                json!({"machine": machine, "start_minute": START}),
                key,
            ),
        )
    };
    let first = order(&mut s, "CNC-XX", "crew-1").expect("first order");
    assert_eq!(first["start_minute"], START);
    assert_eq!(first["complete_minute"], START + minutes);
    let second = order(&mut s, "CNC-XY", "crew-2").expect("second order");
    assert_eq!(
        second["start_minute"],
        START + minutes,
        "one crew: the second machine waits for the first repair"
    );
    assert_eq!(second["complete_minute"], START + 2 * minutes);
    // 同一设备在维修未结束时再下单：拒绝，且不占用编号与班组。
    assert!(
        order(&mut s, "CNC-XX", "crew-3").is_err(),
        "an overlapping order on the same machine is rejected"
    );
    let third = order(&mut s, "CNC-XZ", "crew-4").expect("third order");
    assert_eq!(third["start_minute"], START + 2 * minutes);
    // 重发仍返回原回执。
    assert_eq!(order(&mut s, "CNC-XY", "crew-2").unwrap(), second);
}
