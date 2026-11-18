//! MOM 车间 DES 阶段①（开环）验收用例：RT-DES-01～05、16、17。
//!
//! 用例定义见 `docs/testing/CASES.md`「MOM 车间 DES 阶段①」，设计见 `docs/mom/DES_CLOSED_LOOP.md`。
//! 断言只针对设计文档中约定的业务性质（可复现、因果、报警锁存、标准答案隔离、模式约束、随机流独立），
//! 不依赖具体参数取值。用例定稿后不为迎合实现修改断言。
use an_runtime::mom::des::*;
use an_runtime::mom::{Engine, EventInput, SimulationConfig};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// S1 夹具的起点：08:17 = 第 497 分钟。
const START: u64 = 497;

fn scenario(
    seed: u64,
    pattern: Pattern,
    horizon: u64,
    mechanisms: Option<Mechanisms>,
) -> ScenarioConfig {
    ScenarioConfig {
        seed,
        pattern,
        start_minute: START,
        horizon_minutes: horizon,
        mechanisms,
    }
}

fn run(seed: u64, pattern: Pattern, horizon: u64, mechanisms: Option<Mechanisms>) -> DesRun {
    run_open_loop(
        &FactoryConfig::line_b_default(),
        &scenario(seed, pattern, horizon, mechanisms),
    )
    .expect("scenario should be generated")
}

fn cluster() -> Pattern {
    Pattern::ConcurrentCluster {
        window_minutes: 60,
        min_disruptions: 3,
    }
}

/// 只开启"主轴磨损→尺寸"。
fn spindle_only() -> Mechanisms {
    Mechanisms {
        spindle_wear_to_dimension: true,
        tool_wear_to_dimension: false,
        material_lot_to_dimension: false,
    }
}

/// 三种尺寸机制全关。
fn none() -> Mechanisms {
    Mechanisms::default()
}

fn bytes<T: serde::Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

/// 某设备某类事件，按时间排序。
fn events_of<'a>(r: &'a DesRun, kind: &str, asset: &str) -> Vec<&'a EventInput> {
    let mut v: Vec<_> = r
        .events
        .iter()
        .filter(|e| e.kind == kind && e.asset == asset)
        .collect();
    v.sort_by_key(|e| e.occurred_at);
    v
}

fn truth_of<'a>(r: &'a DesRun, event_id: &str) -> &'a AlarmTruth {
    r.truth
        .alarms
        .iter()
        .find(|a| a.event_id == event_id)
        .unwrap_or_else(|| panic!("alarm {event_id} has no ground truth"))
}

fn machine_ids(r: &DesRun) -> Vec<String> {
    r.telemetry.machines.keys().cloned().collect()
}

#[test]
fn rt_des_01_same_inputs_replay_byte_identical_events_and_truth() {
    for seed in [1, 7, 42] {
        let a = run(seed, cluster(), 240, None);
        let b = run(seed, cluster(), 240, None);
        assert_eq!(
            bytes(&a),
            bytes(&b),
            "seed {seed}: same inputs must replay byte-identical events, truth and telemetry"
        );
        assert!(
            a.events.len() >= 3,
            "seed {seed}: concurrent cluster must produce events, got {}",
            a.events.len()
        );
        assert!(
            !a.truth.alarms.is_empty(),
            "seed {seed}: alarms need ground truth"
        );
        let ids: BTreeSet<_> = a.events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids.len(),
            a.events.len(),
            "seed {seed}: event ids must be unique"
        );
        for e in &a.events {
            assert!(
                (START..=START + 240).contains(&e.occurred_at),
                "seed {seed}: event {} at {} outside window",
                e.id,
                e.occurred_at
            );
            assert!(
                matches!(e.kind.as_str(), "order" | "vibration" | "quality"),
                "phase 1 only emits order/vibration/quality, got {}",
                e.kind
            );
            assert!(
                matches!(e.source.as_str(), "MES" | "EAM" | "QMS"),
                "unexpected source {}",
                e.source
            );
            assert_eq!(
                e.scenario, "s1",
                "events must be accepted by the existing engine"
            );
        }
    }
    let x = run(1, cluster(), 240, None);
    let y = run(2, cluster(), 240, None);
    assert_ne!(
        bytes(&x.events),
        bytes(&y.events),
        "different seeds must produce different event streams"
    );
}

#[test]
fn rt_des_02_spindle_wear_drives_vibration_before_cpk_only_when_mechanism_on() {
    let mut causal_chains = 0;
    for seed in 1..=10 {
        let on = run(seed, Pattern::Unconstrained, 720, Some(spindle_only()));
        let off = run(seed, Pattern::Unconstrained, 720, Some(none()));
        assert!(
            !on.telemetry.machines.is_empty(),
            "seed {seed}: telemetry required"
        );
        assert_eq!(
            on.truth.mechanisms,
            spindle_only(),
            "truth must record the mechanisms used"
        );

        for id in machine_ids(&on) {
            // 主轴磨损轨迹不受尺寸机制开关影响（同一随机流），且只增不减（Gamma 过程，开环无维修）。
            let wear_on = &on.telemetry.machines[&id].spindle_wear;
            let wear_off = &off.telemetry.machines[&id].spindle_wear;
            assert_eq!(
                wear_on, wear_off,
                "seed {seed} {id}: wear must not depend on dimension mechanisms"
            );
            assert!(
                wear_on.len() >= 2,
                "seed {seed} {id}: wear samples required"
            );
            assert!(
                wear_on.windows(2).all(|w| w[1].value >= w[0].value),
                "seed {seed} {id}: spindle wear must be non-decreasing"
            );
            assert!(
                wear_on.last().unwrap().value > wear_on[0].value,
                "seed {seed} {id}: wear must progress over 12 hours"
            );
            // 振动只取决于磨损，机制开关不影响振动报警。
            assert_eq!(
                events_of(&on, "vibration", &id).len(),
                events_of(&off, "vibration", &id).len(),
                "seed {seed} {id}: vibration alarms must not depend on dimension mechanisms"
            );

            // 机制开启：同一设备若出现质量报警，振动报警必须更早，且根因含主轴磨损。
            let quality = events_of(&on, "quality", &id);
            if let Some(q) = quality.first() {
                let vib = events_of(&on, "vibration", &id);
                let v = vib.first().unwrap_or_else(|| {
                    panic!("seed {seed} {id}: Cpk alarm without earlier vibration alarm")
                });
                assert!(
                    v.occurred_at < q.occurred_at,
                    "seed {seed} {id}: vibration alarm must precede Cpk alarm"
                );
                let cause = format!("spindle_wear:{id}");
                assert!(
                    truth_of(&on, &q.id).root_causes.contains(&cause),
                    "seed {seed} {id}: Cpk alarm root cause must include {cause}"
                );
                causal_chains += 1;
            }
        }

        // 机制全关：没有质量报警，Cpk 始终不低于门槛。
        assert!(
            off.events.iter().all(|e| e.kind != "quality"),
            "seed {seed}: no quality alarm may appear without a dimension mechanism"
        );
        for (id, t) in &off.telemetry.machines {
            assert!(!t.cpk.is_empty(), "seed {seed} {id}: Cpk samples required");
            assert!(
                t.cpk.iter().all(|s| s.value >= CPK_THRESHOLD),
                "seed {seed} {id}: Cpk must stay >= {CPK_THRESHOLD} when no mechanism affects dimensions"
            );
        }
    }
    assert!(
        causal_chains > 0,
        "at least one seed must show spindle wear -> vibration -> Cpk alarm within 12 hours"
    );
}

#[test]
fn rt_des_03_alarms_latch_without_recovery() {
    let mut vibration_alarms = 0;
    for seed in 1..=10 {
        let r = run(seed, Pattern::Unconstrained, 1440, Some(spindle_only()));
        assert!(
            !r.telemetry.machines.is_empty(),
            "seed {seed}: telemetry required"
        );
        for id in machine_ids(&r) {
            let v = events_of(&r, "vibration", &id).len();
            let q = events_of(&r, "quality", &id).len();
            assert!(
                v <= 1,
                "seed {seed} {id}: vibration alarm repeated {v} times without recovery"
            );
            assert!(
                q <= 1,
                "seed {seed} {id}: quality alarm repeated {q} times without recovery"
            );
            vibration_alarms += v;
        }
    }
    assert!(
        vibration_alarms > 0,
        "at least one seed must raise a vibration alarm in 24 hours"
    );
}

/// 标准答案中的字段名与隐状态名，不得出现在任何对 agent 或人展示的视图中。
const TRUTH_MARKERS: &[&str] = &[
    "root_causes",
    "mechanisms",
    "ground_truth",
    "spindle_wear",
    "wear_to_dimension",
];

fn assert_no_truth(label: &str, v: &Value) {
    let text = v.to_string();
    for m in TRUTH_MARKERS {
        assert!(!text.contains(m), "{label} leaks ground-truth marker {m}");
    }
}

fn des_config(seed: u64, pattern: Pattern, horizon: u64) -> DesSimulationConfig {
    DesSimulationConfig {
        name: format!("des-{seed}"),
        factory: None,
        scenario: scenario(seed, pattern, horizon, None),
    }
}

#[test]
fn rt_des_04_ground_truth_is_isolated_from_agent_and_human_views() {
    let mut e = Engine::memory().unwrap();
    let sim = e
        .create_des_simulation(des_config(7, cluster(), 120))
        .expect("RT-DES-04: DES simulation must be created");
    e.advance(START + 120).unwrap();

    let ids = e.case_ids();
    assert!(!ids.is_empty(), "DES events must reach the engine as cases");
    let observed: usize = ids
        .iter()
        .map(|id| e.case(id).unwrap().observations.len())
        .sum();
    assert!(
        observed >= 3,
        "concurrent cluster must deliver at least 3 observations, got {observed}"
    );

    let truth = e
        .des_review_truth(&sim)
        .expect("review layer must read ground truth");
    assert!(
        truth.to_string().contains("root_causes"),
        "review truth must contain root causes"
    );

    for id in &ids {
        assert_no_truth(
            &format!("agent view {id}"),
            &e.agent_view(id, 0, 999).unwrap(),
        );
        assert_no_truth(&format!("human view {id}"), &e.human_view(id).unwrap());
    }
    assert_no_truth("simulations view", &e.simulations_view());
}

#[test]
fn rt_des_05_des_simulation_leaves_scripted_simulation_unchanged() {
    let mut a = Engine::memory().unwrap();
    let mut b = Engine::memory().unwrap();
    let ida = a.create_simulation(SimulationConfig::default()).unwrap();
    let idb = b.create_simulation(SimulationConfig::default()).unwrap();
    assert_eq!(ida, idb);
    b.create_des_simulation(des_config(5, cluster(), 60))
        .expect("RT-DES-05: DES simulation must be created next to a scripted one");

    a.advance(560).unwrap();
    b.advance(560).unwrap();

    assert_eq!(
        serde_json::to_value(a.case(&ida).unwrap()).unwrap(),
        serde_json::to_value(b.case(&idb).unwrap()).unwrap(),
        "scripted case must be identical with or without a DES simulation"
    );
    let scripted = |e: &Engine| -> Value {
        e.simulations_view()["simulations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["case_id"] == ida.as_str())
            .cloned()
            .expect("scripted simulation listed")
    };
    assert_eq!(
        scripted(&a),
        scripted(&b),
        "scripted queue must be unchanged"
    );
    let listed = b.simulations_view().to_string();
    assert!(
        listed.contains("des-5"),
        "DES simulation must be listed in simulations view"
    );
}

fn assert_pattern_error(cfg: ScenarioConfig) {
    let factory = FactoryConfig::line_b_default();
    let first = run_open_loop(&factory, &cfg);
    let second = run_open_loop(&factory, &cfg);
    let msg = |r: Result<DesRun, anyhow::Error>| match r {
        Ok(_) => panic!(
            "unsatisfiable pattern {:?} must fail instead of relaxing",
            cfg.pattern
        ),
        Err(e) => e.to_string(),
    };
    let (m1, m2) = (msg(first), msg(second));
    assert!(
        m1.contains("constraint"),
        "error must name the constraint: {m1}"
    );
    assert_eq!(m1, m2, "error must be deterministic");
}

#[test]
fn rt_des_16_scenario_patterns_satisfy_constraints_or_fail_loudly() {
    for seed in 1..=10 {
        // P0 固定剧本：按 S1 时序产生插单、振动、质量报警，根因为 CNC-XX 主轴磨损。
        let p0 = run(seed, Pattern::Script, 30, None);
        let shape: Vec<_> = p0
            .events
            .iter()
            .map(|e| (e.kind.as_str(), e.occurred_at, e.asset.as_str()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("order", START, "CNC-XX"),
                ("vibration", START + 6, "CNC-XX"),
                ("quality", START + 14, "CNC-XX"),
            ],
            "seed {seed}: script pattern must reproduce the S1 timeline"
        );
        assert!(truth_of(&p0, &p0.events[2].id)
            .root_causes
            .contains(&"spindle_wear:CNC-XX".to_string()));

        // P1 并发簇：某 60 分钟窗口内至少 3 个扰动，且至少一对真实关联。
        let p1 = run(seed, cluster(), 240, None);
        let times: Vec<u64> = p1.events.iter().map(|e| e.occurred_at).collect();
        let densest = times
            .iter()
            .map(|t| times.iter().filter(|u| (*t..*t + 60).contains(*u)).count())
            .max()
            .unwrap_or(0);
        assert!(
            densest >= 3,
            "seed {seed}: cluster needs >=3 disruptions in 60 minutes, got {densest}"
        );
        let ids: BTreeSet<_> = p1.events.iter().map(|e| e.id.clone()).collect();
        assert!(
            p1.truth
                .relations
                .iter()
                .any(|r| ids.contains(&r.from_event) && ids.contains(&r.to_event)),
            "seed {seed}: cluster needs at least one true relation between emitted events"
        );

        // P2 诱饵：同一设备 60 分钟内先后出现振动与质量报警，但质量根因不是主轴磨损。
        let p2 = run(seed, Pattern::Decoy, 480, None);
        assert!(
            !p2.truth.mechanisms.spindle_wear_to_dimension,
            "seed {seed}: decoy needs spindle mechanism off"
        );
        let decoy = machine_ids(&p2).into_iter().find_map(|id| {
            let v = events_of(&p2, "vibration", &id)
                .first()
                .map(|e| e.occurred_at)?;
            let q = events_of(&p2, "quality", &id).into_iter().next()?;
            (q.occurred_at.abs_diff(v) <= 60).then(|| (id, q.id.clone()))
        });
        let (id, qid) = decoy.unwrap_or_else(|| {
            panic!(
                "seed {seed}: decoy needs vibration and quality on one machine within 60 minutes"
            )
        });
        let causes = &truth_of(&p2, &qid).root_causes;
        assert!(
            !causes.is_empty(),
            "seed {seed}: decoy quality alarm needs a real cause"
        );
        assert!(
            !causes.contains(&format!("spindle_wear:{id}")),
            "seed {seed}: decoy quality alarm must not be caused by spindle wear"
        );

        // P3 级联：插单 → 振动 → 质量，带负荷加速磨损与共同原因两类关联。
        let p3 = run(seed, Pattern::Cascade, 720, None);
        let rel = |kind: &str| {
            p3.truth
                .relations
                .iter()
                .filter(|r| r.relation == kind)
                .cloned()
                .collect::<Vec<_>>()
        };
        let load = rel("load_accelerates_wear");
        let common = rel("common_cause:spindle_wear");
        assert!(
            !load.is_empty() && !common.is_empty(),
            "seed {seed}: cascade needs both relations"
        );
        let at = |id: &str| {
            p3.events
                .iter()
                .find(|e| e.id == id)
                .map(|e| (e.kind.clone(), e.occurred_at))
        };
        let chained = load.iter().any(|l| {
            common.iter().any(|c| {
                let (o, v, q) = (at(&l.from_event), at(&l.to_event), at(&c.to_event));
                l.to_event == c.from_event
                    && matches!((&o, &v, &q), (Some((ok, ot)), Some((vk, vt)), Some((qk, qt)))
                        if ok == "order" && vk == "vibration" && qk == "quality" && ot < vt && vt < qt)
            })
        });
        assert!(
            chained,
            "seed {seed}: cascade needs order -> vibration -> quality in time order"
        );

        // P4 资源竞争：至少一种资源对应两个以上已发出的事件。
        let p4 = run(seed, Pattern::ResourceContention, 480, None);
        let emitted: BTreeSet<_> = p4.events.iter().map(|e| e.id.clone()).collect();
        assert!(
            p4.truth
                .contentions
                .iter()
                .any(|c| c.event_ids.len() >= 2 && c.event_ids.iter().all(|i| emitted.contains(i))),
            "seed {seed}: resource contention needs a resource shared by >=2 emitted events"
        );

        // P5 安静对照：没有事件，Cpk 始终不低于门槛。
        let p5 = run(seed, Pattern::Quiet, 240, None);
        assert!(
            p5.events.is_empty(),
            "seed {seed}: quiet pattern must not emit events"
        );
        assert!(
            !p5.telemetry.machines.is_empty(),
            "seed {seed}: quiet pattern still needs telemetry"
        );
        for (id, t) in &p5.telemetry.machines {
            assert!(!t.cpk.is_empty(), "seed {seed} {id}: Cpk samples required");
            assert!(
                t.cpk.iter().all(|s| s.value >= CPK_THRESHOLD),
                "seed {seed} {id}: quiet Cpk below threshold"
            );
        }
    }

    // 约束无法满足：报错而不是放宽。
    assert_pattern_error(scenario(
        1,
        Pattern::ConcurrentCluster {
            window_minutes: 10,
            min_disruptions: 50,
        },
        60,
        None,
    ));
    // 诱饵要求质量报警另有原因；只开主轴机制时不可能满足。
    assert_pattern_error(scenario(1, Pattern::Decoy, 480, Some(spindle_only())));
}

#[test]
fn rt_des_17_adding_unrelated_machine_keeps_other_random_streams() {
    let base = FactoryConfig::line_b_default();
    let mut extended = base.clone();
    extended.machines.push(MachineSpec {
        id: "CNC-YY".into(),
        line: "Line-C".into(),
        cycle_seconds: 90,
    });
    let cfg = scenario(11, Pattern::Unconstrained, 480, Some(spindle_only()));
    let a = run_open_loop(&base, &cfg).expect("base factory runs");
    let b = run_open_loop(&extended, &cfg).expect("extended factory runs");

    assert!(
        b.telemetry.machines.contains_key("CNC-YY"),
        "new machine must appear in telemetry"
    );
    let line_b = |r: &DesRun| -> BTreeMap<String, Vec<u8>> {
        ["CNC-XX", "CNC-XY", "CNC-XZ"]
            .iter()
            .map(|id| {
                let t = r
                    .telemetry
                    .machines
                    .get(*id)
                    .unwrap_or_else(|| panic!("{id} missing from telemetry"));
                (id.to_string(), bytes(t))
            })
            .collect()
    };
    assert_eq!(
        line_b(&a),
        line_b(&b),
        "adding an unrelated machine must not change Line-B random streams"
    );
}

// ───────────────────── 阶段②：工具读取、what-if、设备数上限 ─────────────────────

/// 在引擎中创建 P0 固定剧本 DES 仿真（插单 +0、振动 +6、质量 +14，均在 CNC-XX）。
fn p0_engine(factory: Option<FactoryConfig>) -> (Engine, String) {
    let mut e = Engine::memory().unwrap();
    let sim = e
        .create_des_simulation(DesSimulationConfig {
            name: "p0".into(),
            factory,
            scenario: scenario(1, Pattern::Script, 240, None),
        })
        .unwrap();
    (e, sim)
}

/// 引擎可见状态的指纹：事件序号、仿真列表、全部案例。
fn fingerprint(e: &Engine) -> Value {
    let cases: Vec<Value> = e
        .case_ids()
        .iter()
        .map(|id| serde_json::to_value(e.case(id).unwrap()).unwrap())
        .collect();
    serde_json::json!({"sequence": e.sequence(), "simulations": e.simulations_view(), "cases": cases})
}

fn urgent_order() -> WhatIfRequest {
    WhatIfRequest {
        asset: "CNC-XX".into(),
        quantity: 500,
        due_minute: 960,
    }
}

fn plan<'a>(r: &'a Value, id: &str) -> &'a Value {
    r["plans"]
        .as_array()
        .expect("plans array")
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("plan {id} missing"))
}

fn violations(p: &Value) -> Vec<String> {
    p["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn uses(p: &Value) -> Vec<String> {
    p["uses"]
        .as_array()
        .expect("uses array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn rt_des_06_whatif_is_read_only_and_compares_plans() {
    let (mut e, sim) = p0_engine(None);
    e.advance(START + 20).unwrap();
    let before = fingerprint(&e);
    let r1 = e
        .des_whatif(&sim, &urgent_order())
        .expect("RT-DES-06: what-if must answer");
    let r2 = e.des_whatif(&sim, &urgent_order()).unwrap();
    assert_eq!(r1, r2, "what-if must be deterministic");
    assert_eq!(
        before,
        fingerprint(&e),
        "what-if must not change engine state"
    );

    let ids: Vec<&str> = r1["plans"]
        .as_array()
        .expect("plans array")
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["A", "B", "C"], "three candidate plans");
    for id in ["A", "B", "C"] {
        let p = plan(&r1, id);
        for field in [
            "feasible",
            "completion_minute",
            "on_time",
            "risk_parts",
            "violations",
            "uses",
        ] {
            assert!(p.get(field).is_some(), "plan {id} lacks {field}");
        }
    }
    let (a, b, c) = (plan(&r1, "A"), plan(&r1, "B"), plan(&r1, "C"));
    assert!(
        violations(a).contains(&"quality_risk:CNC-XX".to_string()),
        "plan A keeps an alarmed machine and must carry quality_risk"
    );
    assert_eq!(a["feasible"], false, "plan A must not be feasible");
    let risk = |p: &Value| p["risk_parts"].as_f64().unwrap();
    assert!(risk(b) < risk(a), "plan B must risk fewer parts than A");
    assert_eq!(risk(c), 0.0, "plan C stops the alarmed machine at once");
    let rec = r1["recommended"].as_str().expect("a recommended plan");
    assert_eq!(
        plan(&r1, rec)["feasible"],
        true,
        "recommended plan must be feasible"
    );
    assert_eq!(r1["provenance"], "simulation");
    assert_eq!(r1["basis"], "observable_state");
    assert_no_truth("what-if", &r1);
}

#[test]
fn rt_des_07_infeasible_plans_list_hard_constraint_violations() {
    // 默认资源：至少一个方案可行；用到 CNC-XZ 的方案需要先调拨刀具。
    let (mut e, sim) = p0_engine(None);
    e.advance(START + 20).unwrap();
    let r = e
        .des_whatif(&sim, &urgent_order())
        .expect("what-if answers");
    assert!(
        ["B", "C"].iter().any(|id| plan(&r, id)["feasible"] == true),
        "with default resources plan B or C must be feasible"
    );
    let c = plan(&r, "C");
    assert!(
        uses(c).contains(&"CNC-XZ".to_string()),
        "plan C uses CNC-XZ"
    );
    assert!(
        c["prerequisites"]
            .as_array()
            .expect("prerequisites array")
            .iter()
            .any(|p| p == "tool_transfer:CNC-XZ"),
        "using CNC-XZ requires a tool transfer first"
    );

    // 没有备用刀具：CNC-XZ 不可用，只剩 CNC-XY，误期。
    let mut no_spare = FactoryConfig::line_b_default();
    no_spare.resources.tool_spares = 0;
    // 未认证：CNC-XZ 不可用于该产品。
    let mut uncertified = FactoryConfig::line_b_default();
    uncertified.resources.uncertified = vec!["CNC-XZ".into()];
    for (factory, expected) in [
        (no_spare, "tool_unavailable:CNC-XZ"),
        (uncertified, "uncertified:CNC-XZ"),
    ] {
        let (mut e, sim) = p0_engine(Some(factory));
        e.advance(START + 20).unwrap();
        let r = e
            .des_whatif(&sim, &urgent_order())
            .expect("what-if answers");
        for id in ["B", "C"] {
            let p = plan(&r, id);
            let v = violations(p);
            assert!(
                v.contains(&expected.to_string()),
                "plan {id} must list {expected}: {v:?}"
            );
            assert!(
                !uses(p).contains(&"CNC-XZ".to_string()),
                "plan {id} must not use CNC-XZ"
            );
            assert!(
                v.contains(&"deadline_miss".to_string()),
                "plan {id} misses the deadline: {v:?}"
            );
            assert_eq!(p["feasible"], false, "plan {id} must be infeasible");
        }
    }
}

#[test]
fn rt_des_20_tools_in_des_cases_read_simulation_state() {
    let (mut e, _sim) = p0_engine(None);
    e.advance(START + 30).unwrap();
    let id = e
        .case_ids()
        .into_iter()
        .find(|id| !e.case(id).unwrap().template.is_empty())
        .expect("P0 events must form one planned case");
    let c = e.case(&id).unwrap();
    let output = |node: &str| -> Value {
        let n = &c.nodes[node];
        assert_eq!(n.status, "SUCCEEDED", "node {node} must succeed");
        let ev = &c.evidence[&n.evidence_refs[0]];
        assert_eq!(ev["source"], "DES", "node {node} must read DES");
        assert_eq!(ev["provenance"], "simulation", "node {node} provenance");
        assert!(
            ev["mock_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == "M27"),
            "node {node} must cite M27"
        );
        n.output.clone().expect("node output")
    };
    let m = output("maintenance");
    assert!(
        m["vibration_mm_s"].as_f64().unwrap() >= 2.8,
        "vibration in zone C"
    );
    assert_eq!(m["zone"], "C");
    let q = output("quality");
    assert!(
        q["cpk"].as_f64().unwrap() < CPK_THRESHOLD,
        "Cpk below threshold"
    );
    let r = output("resources");
    let xz = r["machines"]
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["id"] == "CNC-XZ")
        .expect("CNC-XZ listed")
        .clone();
    assert_eq!(xz["tool_present"], false, "CNC-XZ lacks the tool");
    let s = output("schedule");
    assert_eq!(
        s["plans"].as_array().map(|p| p.len()),
        Some(3),
        "three plans"
    );
    assert!(s["recommended"].is_string(), "a recommended plan");
    let proposal = c.proposal.as_ref().expect("proposal");
    assert_eq!(proposal["plans"].as_array().map(|p| p.len()), Some(3));
    assert_no_truth("agent view", &e.agent_view(&id, 0, 999).unwrap());
    assert_no_truth("human view", &e.human_view(&id).unwrap());
}

#[test]
fn rt_des_21_factory_size_is_bounded() {
    let factory = |n: usize| {
        let mut f = FactoryConfig::line_b_default();
        while f.machines.len() < n {
            let i = f.machines.len();
            f.machines.push(MachineSpec {
                id: format!("CNC-{i:02}"),
                line: "Line-B".into(),
                cycle_seconds: 90,
            });
        }
        f
    };
    let cfg = scenario(1, Pattern::Unconstrained, 60, None);
    run_open_loop(&factory(MAX_MACHINES), &cfg).expect("32 machines must run");
    let Err(err) = run_open_loop(&factory(MAX_MACHINES + 1), &cfg) else {
        panic!("33 machines must be rejected");
    };
    assert!(
        err.to_string().contains("machines"),
        "error must name the limit: {err}"
    );
    let mut e = Engine::memory().unwrap();
    assert!(
        e.create_des_simulation(DesSimulationConfig {
            name: "too-big".into(),
            factory: Some(factory(MAX_MACHINES + 1)),
            scenario: cfg,
        })
        .is_err(),
        "engine must reject oversized factories"
    );
}

// ───────────────────── 阶段③：方案选定、执行回写、核验 ─────────────────────

/// 推进 P0 仿真直到 DES 案例进入人工决策，返回案例编号。
fn p0_waiting(e: &mut Engine) -> String {
    for t in START..=START + 30 {
        e.advance(t).unwrap();
        if let Some(id) = e
            .case_ids()
            .into_iter()
            .find(|id| e.case(id).unwrap().stage == "WAITING_HUMAN")
        {
            return id;
        }
    }
    panic!("P0 case must reach WAITING_HUMAN within 30 minutes");
}

/// 选定方案、批准并执行，返回执行时刻。
fn run_plan(e: &mut Engine, id: &str, plan_id: &str) -> u64 {
    e.select_des_plan(id, plan_id)
        .unwrap_or_else(|err| panic!("plan {plan_id} must be selectable: {err}"));
    let rev = e.case(id).unwrap().plan_revision;
    e.approve(id, rev, "production", "supervisor").unwrap();
    e.execute(id, "production").unwrap();
    e.now()
}

fn action<'a>(e: &'a Engine, id: &str, tool: &str) -> &'a an_runtime::mom::Action {
    e.case(id)
        .unwrap()
        .actions
        .values()
        .find(|a| a.tool == tool)
        .unwrap_or_else(|| panic!("action {tool} missing"))
}

fn machine(view: &Value, id: &str) -> Value {
    view["machines"]
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["id"] == id)
        .unwrap_or_else(|| panic!("{id} missing"))
        .clone()
}

fn event_kinds_after(e: &Engine, id: &str, after: u64) -> Vec<String> {
    e.events(id, after, 1000)
        .unwrap()
        .into_iter()
        .map(|ev| ev.kind)
        .collect()
}

#[test]
fn rt_des_08_selected_plan_b_changes_the_simulated_world() {
    let (mut e, sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    assert!(
        e.select_des_plan(&id, "A").is_err(),
        "infeasible plan A must not be selectable"
    );
    let t0 = run_plan(&mut e, &id, "B");
    let r = FactoryConfig::line_b_default().resources;
    let maintenance_end = t0 + r.finish_batch_minutes + r.maintenance_minutes;
    assert_eq!(
        action(&e, &id, "eam.maintenance").complete_at,
        maintenance_end,
        "maintenance completes after finishing the batch and the repair"
    );
    assert_eq!(
        action(&e, &id, "mro.transfer").complete_at,
        t0 + r.tool_transfer_minutes,
        "tool transfer completes after the transfer time"
    );

    // 维修期间 CNC-XX 停机；调拨到达前 CNC-XZ 无刀。
    e.advance(t0 + r.finish_batch_minutes + 10).unwrap();
    let f = e.des_factory_view(&sim).unwrap();
    assert_eq!(
        machine(&f, "CNC-XX")["status"],
        "stopped",
        "CNC-XX stopped for maintenance"
    );
    // 调拨到达后 CNC-XZ 有刀。
    e.advance(t0 + r.tool_transfer_minutes + 1).unwrap();
    let f = e.des_factory_view(&sim).unwrap();
    assert_eq!(
        machine(&f, "CNC-XZ")["tool_present"],
        true,
        "tool arrived at CNC-XZ"
    );
    assert_eq!(action(&e, &id, "mro.transfer").state, "VERIFIED");

    // 维修完成后 CNC-XX 恢复。
    e.advance(maintenance_end + 5).unwrap();
    let f = e.des_factory_view(&sim).unwrap();
    let xx = machine(&f, "CNC-XX");
    assert_eq!(xx["status"], "running", "CNC-XX back in service");
    assert!(
        xx["vibration_mm_s"].as_f64().unwrap() < 2.8,
        "vibration recovered: {xx}"
    );
    assert!(
        xx["cpk"].as_f64().unwrap() >= CPK_THRESHOLD,
        "Cpk recovered: {xx}"
    );
    assert!(
        e.case(&id)
            .unwrap()
            .actions
            .values()
            .all(|a| a.state == "VERIFIED"),
        "all actions verified"
    );
    e.close(&id).expect("verified case can be closed");
    assert_eq!(e.case(&id).unwrap().stage, "CLOSED");
}

#[test]
fn rt_des_09_post_execution_same_source_observation_is_verification_evidence() {
    let (mut e, _sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    run_plan(&mut e, &id, "C");
    let before = e.case(&id).unwrap().clone();
    let seq = e.sequence();
    let mut recheck = before
        .observations
        .iter()
        .find(|o| o.event.kind == "quality")
        .expect("quality observation")
        .event
        .clone();
    recheck.id = format!("{}-recheck", recheck.id);
    recheck.occurred_at = e.now();
    recheck.payload["cpk"] = serde_json::json!(1.4);
    assert_eq!(
        e.ingest(recheck).unwrap(),
        id,
        "re-check joins the same case"
    );
    let after = e.case(&id).unwrap();
    let kinds = event_kinds_after(&e, &id, seq);
    assert!(
        kinds.iter().any(|k| k == "VerificationObservationRecorded"),
        "re-check must be recorded as verification evidence: {kinds:?}"
    );
    assert!(
        !kinds.iter().any(|k| k == "PlanInvalidated"),
        "re-check must not invalidate the plan: {kinds:?}"
    );
    assert_eq!(after.plan_revision, before.plan_revision);
    assert_eq!(after.stage, before.stage);
    assert_eq!(after.observations.len(), before.observations.len() + 1);
}

#[test]
fn rt_des_10_new_disturbance_after_execution_still_invalidates_plan() {
    let (mut e, _sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    run_plan(&mut e, &id, "C");
    let seq = e.sequence();
    let mut order = e
        .case(&id)
        .unwrap()
        .observations
        .iter()
        .find(|o| o.event.kind == "order")
        .expect("order observation")
        .event
        .clone();
    order.id = format!("{}-second", order.id);
    order.occurred_at = e.now();
    order.payload["quantity"] = serde_json::json!(200);
    assert_eq!(e.ingest(order).unwrap(), id);
    let kinds = event_kinds_after(&e, &id, seq);
    assert!(
        kinds.iter().any(|k| k == "PlanInvalidated"),
        "a new order must still invalidate the plan: {kinds:?}"
    );
}

#[test]
fn rt_des_11_tool_transfer_failure_comes_from_the_simulation() {
    let (mut e, sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    e.select_des_plan(&id, "C").expect("plan C selectable");
    let rev = e.case(&id).unwrap().plan_revision;
    e.approve(&id, rev, "production", "supervisor").unwrap();
    e.des_inject_fault(&sim, DesFault::ToolSparesLost)
        .expect("fault injection");
    let seq = e.sequence();
    e.execute(&id, "production").unwrap();
    assert_eq!(
        action(&e, &id, "mro.transfer").state,
        "FAILED",
        "transfer fails without spares"
    );
    assert_eq!(e.case(&id).unwrap().stage, "PARTIAL");
    assert!(
        event_kinds_after(&e, &id, seq)
            .iter()
            .any(|k| k == "CompensationRequired"),
        "failure requires compensation"
    );
    let t0 = e.now();
    e.advance(t0 + 60).unwrap();
    let f = e.des_factory_view(&sim).unwrap();
    assert_eq!(
        machine(&f, "CNC-XZ")["tool_present"],
        false,
        "no tool arrived"
    );
}

#[test]
fn rt_des_18_cases_sharing_resources_are_linked() {
    let mut e = Engine::memory().unwrap();
    let scripted = e.create_simulation(SimulationConfig::default()).unwrap();
    e.create_des_simulation(des_config(1, Pattern::ResourceContention, 240))
        .unwrap();
    e.advance(START + 240).unwrap();
    let alarm_cases: Vec<String> = e
        .case_ids()
        .into_iter()
        .filter(|id| {
            let c = e.case(id).unwrap();
            c.batch.starts_with("DES-")
                && c.observations
                    .iter()
                    .any(|o| matches!(o.event.kind.as_str(), "vibration" | "quality"))
        })
        .collect();
    let assets: BTreeSet<String> = alarm_cases
        .iter()
        .map(|id| e.case(id).unwrap().asset.clone())
        .collect();
    assert!(
        assets.len() >= 2,
        "contention needs alarms on two machines: {assets:?}"
    );
    for id in &alarm_cases {
        let links: Vec<Value> = e
            .events(id, 0, 1000)
            .unwrap()
            .into_iter()
            .filter(|ev| ev.kind == "RelatedCaseLinked")
            .map(|ev| ev.payload)
            .collect();
        let others: BTreeSet<String> = links
            .iter()
            .filter_map(|p| p["related_case"].as_str().map(String::from))
            .collect();
        assert!(
            alarm_cases.iter().any(|o| o != id && others.contains(o)),
            "case {id} must be linked to another alarm case: {links:?}"
        );
        assert!(
            !others.contains(&scripted),
            "scripted case must not be linked"
        );
        assert!(
            links.iter().all(|p| p["reason"]
                .as_str()
                .is_some_and(|r| r.contains("maintenance_crew"))),
            "link reason must name the shared resource: {links:?}"
        );
    }
}

// ───────────────────── 阶段④：审核层与批量实验 ─────────────────────

use an_runtime::mom::des::batch::{run_batch, to_csv, ApproverPolicy, BatchConfig, DecisionPolicy};
use an_runtime::mom::des::review::*;

fn rule<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["rules"]
        .as_array()
        .expect("rules array")
        .iter()
        .find(|r| r["rule"] == id)
        .unwrap_or_else(|| panic!("rule {id} missing: {report}"))
}

fn rule_in<'a>(rules: &'a [RuleResult], id: &str) -> &'a RuleResult {
    rules
        .iter()
        .find(|r| r.rule == id)
        .unwrap_or_else(|| panic!("rule {id} missing: {rules:?}"))
}

#[test]
fn rt_des_12_rule_checks_detect_wrong_handling() {
    // 根因假设：错误的被判为误判，正确的通过。
    let (mut e, _sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    e.record_cause_claim(&id, vec!["material_lot:Line-B-LOT-1".into()], "master")
        .expect("cause claims are recorded");
    let report = e.des_review_report(&id).expect("review report");
    let r = rule(&report, RULE_ROOT_CAUSE);
    assert_eq!(r["status"], "fail", "a wrong cause must fail: {r}");
    assert!(
        r["detail"].as_str().unwrap().contains("misattributed"),
        "{r}"
    );
    e.record_cause_claim(&id, vec!["spindle_wear:CNC-XX".into()], "master")
        .unwrap();
    let report = e.des_review_report(&id).unwrap();
    assert_eq!(rule(&report, RULE_ROOT_CAUSE)["status"], "pass");

    // 人为构造的记录：先执行后批准必须被检出；先批准后执行通过。
    let entry = |seq: u64, kind: &str| TimelineEntry {
        seq,
        kind: kind.into(),
        at: 500 + seq,
    };
    let mut record = e.des_review_record(&id).expect("review record");
    record.timeline = vec![entry(1, "ActionAccepted"), entry(2, "DecisionApproved")];
    let truth: GroundTruth = serde_json::from_value(e.des_review_truth("DES-1").unwrap()).unwrap();
    assert_eq!(
        rule_in(&check_rules(&record, &truth), RULE_HITL_BEFORE_WRITE).status,
        RuleStatus::Fail,
        "a write before approval must fail"
    );
    record.timeline = vec![entry(1, "DecisionApproved"), entry(2, "ActionAccepted")];
    assert_eq!(
        rule_in(&check_rules(&record, &truth), RULE_HITL_BEFORE_WRITE).status,
        RuleStatus::Pass
    );

    // 效果核验：维修完成前待定，完成且设备恢复后通过。
    let t0 = run_plan(&mut e, &id, "C");
    let end = t0
        + FactoryConfig::line_b_default()
            .resources
            .maintenance_minutes;
    e.advance(t0 + 10).unwrap();
    let report = e.des_review_report(&id).unwrap();
    assert_eq!(rule(&report, RULE_EFFECTIVENESS)["status"], "pending");
    assert_eq!(rule(&report, RULE_PLAN_SAFE)["status"], "pass");
    e.advance(end + 5).unwrap();
    let report = e.des_review_report(&id).unwrap();
    assert_eq!(
        rule(&report, RULE_EFFECTIVENESS)["status"],
        "pass",
        "{report}"
    );
    assert_eq!(rule(&report, RULE_HITL_BEFORE_WRITE)["status"], "pass");
}

fn reviewer(response: Result<Value, String>, delay_ms: u64) -> ScriptedReviewer {
    ScriptedReviewer {
        choice: ReviewerChoice {
            model: Some("scripted".into()),
            source: "mom_reviewer".into(),
            independent: true,
            label: None,
        },
        response,
        delay: std::time::Duration::from_millis(delay_ms),
    }
}

/// 已执行方案 C 的 P0 引擎（放在锁里，供异步审核使用）。
fn executed_case() -> (std::sync::Mutex<Engine>, String) {
    let (mut e, _sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    run_plan(&mut e, &id, "C");
    (std::sync::Mutex::new(e), id)
}

fn case_state(e: &Engine, id: &str) -> Value {
    serde_json::json!({"case": e.case(id).unwrap(), "simulations": e.simulations_view()})
}

#[tokio::test]
async fn rt_des_13_ai_review_cites_evidence_or_degrades() {
    // 合规结论：引用输入中的证据编号。
    let (engine, id) = executed_case();
    let input = engine
        .lock()
        .unwrap()
        .des_ai_review_input(&id)
        .expect("AI review input");
    let refs: Vec<String> = input["evidence_ids"]
        .as_array()
        .expect("evidence ids")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(!refs.is_empty(), "input must list citable evidence");
    let before = case_state(&engine.lock().unwrap(), &id);
    let ok = reviewer(
        Ok(
            serde_json::json!({"conclusions":[{"judgement":"reasonable","statement":"stopped the alarmed machine","evidence_refs":[refs[0]]}],"summary":"ok"}),
        ),
        0,
    );
    let report = run_ai_review(&engine, &id, &ok, std::time::Duration::from_secs(5))
        .await
        .expect("AI review runs");
    assert_eq!(report["ai"]["status"], "completed", "{report}");
    for c in report["ai"]["conclusions"].as_array().expect("conclusions") {
        let cited = c["evidence_refs"].as_array().unwrap();
        assert!(!cited.is_empty(), "every conclusion cites evidence");
        assert!(
            cited.iter().all(|r| refs.iter().any(|k| r == k)),
            "cited evidence must exist"
        );
    }
    assert_eq!(
        before,
        case_state(&engine.lock().unwrap(), &id),
        "review must not change the case"
    );

    // 报错、超时、引用不存在的证据：AI 不可用，规则结果照常。
    let cases = [
        (reviewer(Err("model offline".into()), 0), "model offline"),
        (
            reviewer(Ok(serde_json::json!({"conclusions":[]})), 300),
            "timeout",
        ),
        (
            reviewer(
                Ok(
                    serde_json::json!({"conclusions":[{"judgement":"wrong","statement":"x","evidence_refs":["event:999999"]}]}),
                ),
                0,
            ),
            "invalid_output",
        ),
    ];
    for (r, reason) in cases {
        let (engine, id) = executed_case();
        let report = run_ai_review(&engine, &id, &r, std::time::Duration::from_millis(100))
            .await
            .expect("degraded review still returns a report");
        assert_eq!(report["ai"]["status"], "unavailable", "{report}");
        assert!(
            report["ai"]["reason"]
                .as_str()
                .is_some_and(|s| s.contains(reason)),
            "reason must mention {reason}: {report}"
        );
        assert!(
            report["rules"].as_array().is_some_and(|r| !r.is_empty()),
            "rules are still reported"
        );
    }
}

#[tokio::test]
async fn rt_des_14_human_override_is_recorded_as_disagreement() {
    let (engine, id) = executed_case();
    let input = engine.lock().unwrap().des_ai_review_input(&id).unwrap();
    let first = input["evidence_ids"][0]
        .as_str()
        .expect("evidence id")
        .to_string();
    let ok = reviewer(
        Ok(
            serde_json::json!({"conclusions":[{"judgement":"reasonable","statement":"fine","evidence_refs":[first]}],"summary":"fine"}),
        ),
        0,
    );
    run_ai_review(&engine, &id, &ok, std::time::Duration::from_secs(5))
        .await
        .unwrap();
    let mut e = engine.lock().unwrap();
    assert!(
        e.des_human_review(&id, "override", "", "supervisor")
            .is_err(),
        "override without a reason must be rejected"
    );
    e.des_human_review(
        &id,
        "override",
        "plan C idles CNC-XX too long",
        "supervisor",
    )
    .expect("override with a reason");
    let report = e.des_review_report(&id).unwrap();
    assert_eq!(report["human"]["decision"], "override");
    let disagreements = report["disagreements"].as_array().expect("disagreements");
    assert_eq!(disagreements.len(), 1);
    assert!(disagreements[0].to_string().contains("idles CNC-XX"));
    e.des_human_review(&id, "agree", "", "supervisor").unwrap();
    let report = e.des_review_report(&id).unwrap();
    assert_eq!(
        report["disagreements"].as_array().unwrap().len(),
        1,
        "agreeing adds no disagreement"
    );
}

#[test]
fn rt_des_15_batch_runs_are_reproducible_and_export_csv() {
    let cfg = BatchConfig {
        factory: None,
        pattern: Pattern::Script,
        horizon_minutes: 60,
        seeds: (1..=100).collect(),
        policies: vec![DecisionPolicy::AiNative, DecisionPolicy::Traditional],
        approver: ApproverPolicy::default(),
    };
    let a = run_batch(&cfg).expect("batch runs");
    let b = run_batch(&cfg).expect("batch runs again");
    assert_eq!(a, b, "batch results must be reproducible");
    assert_eq!(a.len(), 200, "one row per seed and policy");
    for seed in 1..=100 {
        for p in [DecisionPolicy::AiNative, DecisionPolicy::Traditional] {
            assert!(
                a.iter().any(|r| r.seed == seed && r.policy == p),
                "seed {seed} {p:?} missing"
            );
        }
    }
    let csv = to_csv(&a);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 201, "header plus 200 rows");
    assert!(lines[0].starts_with("seed,policy,"), "header: {}", lines[0]);
    let mean = |p: DecisionPolicy| {
        let v: Vec<f64> = a
            .iter()
            .filter(|r| r.policy == p)
            .filter_map(|r| r.mean_decision_minutes)
            .collect();
        assert!(!v.is_empty(), "{p:?} must make decisions");
        v.iter().sum::<f64>() / v.len() as f64
    };
    assert!(
        mean(DecisionPolicy::Traditional) > mean(DecisionPolicy::AiNative),
        "traditional coordination takes longer to decide"
    );
}

#[test]
fn rt_des_19_reviewer_model_choice_and_independence() {
    let base = "[llm]\nbase_url = \"http://127.0.0.1:1/v1\"\nmodel = \"local\"\n";
    let parse = |extra: &str| {
        let (cfg, unknown) =
            an_runtime::config::AppConfig::parse(&format!("{base}{extra}")).expect("config parses");
        assert!(
            unknown.is_empty(),
            "mom_reviewer must be a known field: {unknown:?}"
        );
        resolve_reviewer(&cfg)
    };
    let own = parse("[models]\nmom_reviewer = \"big\"\n");
    assert_eq!(own.model.as_deref(), Some("big"));
    assert_eq!(own.source, "mom_reviewer");
    assert!(
        own.independent,
        "a different model is an independent review"
    );
    assert!(own.label.is_none());
    for c in [parse(""), parse("[models]\nmom_reviewer = \"local\"\n")] {
        assert_eq!(c.model.as_deref(), Some("local"));
        assert!(
            !c.independent,
            "the master's model is not an independent reviewer"
        );
        assert!(c.label.is_some(), "non-independent review must be labelled");
    }
}

// ───────────────────── 阶段③b：按报警触发的模板与产线协同事项 ─────────────────────

const S1_TEMPLATE: &str = "DAG-TPL-COMPOSITE-MAINT-QUAL-RESCHEDULE@1";

fn run_des(pattern: Pattern, seed: u64, horizon: u64) -> Engine {
    let mut e = Engine::memory().unwrap();
    e.create_des_simulation(des_config(seed, pattern, horizon))
        .unwrap();
    e.advance(START + horizon).unwrap();
    e
}

fn is_incident(c: &an_runtime::mom::Case) -> bool {
    c.world.get("des_incident").is_some_and(Value::is_object)
}

fn is_open(c: &an_runtime::mom::Case) -> bool {
    !matches!(c.stage.as_str(), "CLOSED" | "CANCELLED")
}

fn kinds(c: &an_runtime::mom::Case) -> BTreeSet<String> {
    c.observations
        .iter()
        .map(|o| o.event.kind.clone())
        .collect()
}

fn alarm_cases(e: &Engine) -> Vec<an_runtime::mom::Case> {
    e.case_ids()
        .into_iter()
        .map(|id| e.case(&id).unwrap().clone())
        .filter(|c| c.batch.starts_with("DES-") && !is_incident(c))
        .filter(|c| {
            let k = kinds(c);
            k.contains("vibration") || k.contains("quality")
        })
        .collect()
}

fn incidents(e: &Engine) -> Vec<an_runtime::mom::Case> {
    e.case_ids()
        .into_iter()
        .map(|id| e.case(&id).unwrap().clone())
        .filter(is_incident)
        .collect()
}

#[test]
fn rt_des_25_every_alarm_case_gets_a_plan_or_joins_an_incident() {
    let mut checked = 0;
    for (pattern, horizon) in [
        (cluster(), 240),
        (Pattern::ResourceContention, 480),
        (Pattern::Unconstrained, 480),
    ] {
        for seed in 1..=5 {
            let e = run_des(pattern.clone(), seed, horizon);
            for c in alarm_cases(&e).iter().filter(|c| is_open(c)) {
                checked += 1;
                let k = kinds(c);
                assert!(
                    !c.template.is_empty(),
                    "{pattern:?} seed {seed}: {} has no plan ({k:?})",
                    c.id
                );
                if c.world.get("des_member_of").is_some_and(Value::is_string) {
                    assert_eq!(
                        c.template, "DAG-TPL-DES-DIAGNOSIS@1",
                        "member {} uses diagnosis",
                        c.id
                    );
                    assert!(
                        !c.nodes.contains_key("schedule"),
                        "member {} must not schedule",
                        c.id
                    );
                } else {
                    let (v, q, o) = (
                        k.contains("vibration"),
                        k.contains("quality"),
                        k.contains("order"),
                    );
                    let expected = match (v, q, o) {
                        (true, true, true) => S1_TEMPLATE,
                        (true, true, false) => "DAG-TPL-DES-COMBINED@1",
                        (true, false, _) => "DAG-TPL-DES-EQUIPMENT@1",
                        _ => "DAG-TPL-DES-QUALITY@1",
                    };
                    assert_eq!(
                        c.template, expected,
                        "{pattern:?} seed {seed}: {} kinds {k:?}",
                        c.id
                    );
                    assert!(c.nodes.contains_key("schedule"), "{} must schedule", c.id);
                }
            }
        }
    }
    assert!(checked > 0, "the patterns must produce alarm cases");
}

#[test]
fn rt_des_26_line_incident_coordinates_concurrent_cases() {
    let mut found = 0;
    for seed in 1..=5 {
        let mut e = Engine::memory().unwrap();
        e.create_des_simulation(des_config(seed, cluster(), 240))
            .unwrap();
        // 逐分钟检查：同一产线任一时刻最多一个活动事项。
        for t in START..=START + 240 {
            e.advance(t).unwrap();
            let active = incidents(&e).iter().filter(|i| is_open(i)).count();
            assert!(
                active <= 1,
                "seed {seed} minute {t}: {active} active incidents on Line-B"
            );
        }
        // 两个以上 DES 案例的首次观测在 60 分钟内 → 必须有事项。
        let mut firsts: Vec<u64> = e
            .case_ids()
            .into_iter()
            .map(|id| e.case(&id).unwrap().clone())
            .filter(|c| c.batch.starts_with("DES-") && !is_incident(c))
            .filter_map(|c| c.observations.iter().map(|o| o.event.occurred_at).min())
            .collect();
        firsts.sort();
        let close_pair = firsts.windows(2).any(|w| w[1] - w[0] <= 60);
        let incs = incidents(&e);
        if close_pair {
            assert!(
                !incs.is_empty(),
                "seed {seed}: concurrent cases on one line need an incident"
            );
        }
        for inc in &incs {
            found += 1;
            assert_eq!(inc.template, "DAG-TPL-DES-LINE-INCIDENT@1");
            let members: Vec<String> = inc.world["des_incident"]["members"]
                .as_array()
                .expect("members")
                .iter()
                .map(|m| m.as_str().unwrap().to_string())
                .collect();
            assert!(members.len() >= 2, "incident {} needs two members", inc.id);
            let mut member_assets = BTreeSet::new();
            for m in &members {
                let mc = e.case(m).unwrap();
                assert_eq!(
                    mc.world["des_member_of"],
                    inc.id.as_str(),
                    "member {m} points back"
                );
                assert!(
                    !mc.nodes.contains_key("schedule"),
                    "member {m} must not schedule"
                );
                // 方案 A（2026-11-17 用户批准）：加入前已请人决策的成员，决策请求必须在加入时撤回；
                // 加入后不再请人决策。
                let log = e.events(m, 0, 1000).unwrap();
                let joined_at = log
                    .iter()
                    .position(|ev| ev.kind == "JoinedLineIncident")
                    .unwrap_or_else(|| panic!("member {m} records joining"));
                if log[..joined_at]
                    .iter()
                    .any(|ev| ev.kind == "DecisionRequested")
                {
                    assert!(
                        log[..=joined_at]
                            .iter()
                            .any(|ev| ev.kind == "DecisionWithdrawn"),
                        "member {m} decided before joining: its request must be withdrawn"
                    );
                }
                assert!(
                    !log[joined_at..]
                        .iter()
                        .any(|ev| ev.kind == "DecisionRequested"),
                    "member {m} must not request a decision after joining"
                );
                if kinds(mc).iter().any(|k| k == "vibration" || k == "quality") {
                    member_assets.insert(mc.asset.clone());
                }
            }
            if let Some(p) = &inc.proposal {
                assert_eq!(
                    p["plans"].as_array().map(|x| x.len()),
                    Some(3),
                    "incident has three plans"
                );
                let assets: Vec<String> = p["assets"]
                    .as_array()
                    .expect("incident plan names its alarmed machines")
                    .iter()
                    .map(|a| a.as_str().unwrap().to_string())
                    .collect();
                assert!(!assets.is_empty());
                assert!(
                    assets.iter().all(|a| member_assets.contains(a)),
                    "{assets:?} ⊆ {member_assets:?}"
                );
            }
        }
    }
    assert!(
        found > 0,
        "the cluster pattern must produce at least one line incident"
    );
}

#[test]
fn rt_des_27_incident_decision_applies_to_all_alarmed_machines() {
    for seed in 1..=10 {
        let mut e = Engine::memory().unwrap();
        let sim = e
            .create_des_simulation(des_config(seed, Pattern::ResourceContention, 480))
            .unwrap();
        let mut target = None;
        for t in START..=START + 480 {
            e.advance(t).unwrap();
            target = incidents(&e).into_iter().find(|i| {
                i.stage == "WAITING_HUMAN"
                    && i.proposal.as_ref().is_some_and(|p| {
                        p["recommended"].is_string()
                            && p["assets"].as_array().is_some_and(|a| a.len() >= 2)
                    })
            });
            if target.is_some() {
                break;
            }
        }
        let Some(inc) = target else { continue };
        let plan_id = inc.proposal.as_ref().unwrap()["recommended"]
            .as_str()
            .unwrap()
            .to_string();
        let assets: Vec<String> = inc.proposal.as_ref().unwrap()["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        let seq = e.sequence();
        run_plan(&mut e, &inc.id, &plan_id);
        let applied = e
            .events(&inc.id, seq, 1000)
            .unwrap()
            .into_iter()
            .find(|ev| ev.kind == "DesPlanApplied")
            .expect("plan applied to the simulation");
        let windows: Vec<(String, u64, u64)> = applied.payload["interventions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|iv| iv["kind"] == "maintenance")
            .map(|iv| {
                (
                    iv["machine"].as_str().unwrap().to_string(),
                    iv["start_minute"].as_u64().unwrap(),
                    iv["end_minute"].as_u64().unwrap(),
                )
            })
            .collect();
        let maintained: BTreeSet<&String> = windows.iter().map(|w| &w.0).collect();
        assert!(
            assets.iter().all(|a| maintained.contains(a)),
            "every alarmed machine is maintained: {windows:?} vs {assets:?}"
        );
        let mut sorted = windows.clone();
        sorted.sort_by_key(|w| w.1);
        assert!(
            sorted.windows(2).all(|w| w[1].1 >= w[0].2),
            "one crew maintains machines one after another: {sorted:?}"
        );
        let last_end = sorted.iter().map(|w| w.2).max().unwrap();
        e.advance(last_end + 5).unwrap();
        let view = e.des_factory_view(&sim).unwrap();
        for a in &assets {
            let m = machine(&view, a);
            assert_eq!(m["status"], "running", "{a} back in service: {m}");
            assert!(
                m["vibration_mm_s"].as_f64().unwrap() < 2.8,
                "{a} recovered: {m}"
            );
        }
        e.close(&inc.id).expect("verified incident can be closed");
        for m in inc.world["des_incident"]["members"].as_array().unwrap() {
            let mc = e.case(m.as_str().unwrap()).unwrap();
            assert_eq!(
                mc.stage, "CLOSED",
                "member {} closes with the incident",
                mc.id
            );
        }
        return;
    }
    panic!("resource contention must yield an incident with two alarmed machines and a recommended plan within seeds 1–10");
}

#[test]
fn rt_des_28_p0_keeps_single_case_without_incident() {
    let (mut e, _sim) = p0_engine(None);
    e.advance(START + 30).unwrap();
    let ids = e.case_ids();
    assert_eq!(ids.len(), 1, "P0 forms one case");
    let c = e.case(&ids[0]).unwrap();
    assert_eq!(c.template, S1_TEMPLATE);
    assert!(!is_incident(c));
    assert!(c.world.get("des_member_of").is_none_or(Value::is_null));
}

#[test]
fn rt_des_29_closed_des_case_is_not_reused_or_refreshed() {
    let (mut e, _sim) = p0_engine(None);
    let id = p0_waiting(&mut e);
    let t0 = run_plan(&mut e, &id, "C");
    let end = t0
        + FactoryConfig::line_b_default()
            .resources
            .maintenance_minutes;
    e.advance(end + 5).unwrap();
    e.close(&id).unwrap();
    let frozen = e.case(&id).unwrap().world["des"].clone();
    let mut later = e
        .case(&id)
        .unwrap()
        .observations
        .iter()
        .find(|o| o.event.kind == "quality")
        .unwrap()
        .event
        .clone();
    later.id = format!("{}-after-close", later.id);
    later.occurred_at = e.now();
    let new_case = e.ingest(later).unwrap();
    assert_ne!(new_case, id, "a closed case must not receive new events");
    e.advance(e.now() + 10).unwrap();
    assert_eq!(
        e.case(&id).unwrap().world["des"],
        frozen,
        "closed case is not refreshed"
    );
}

// ───────────────────── 阶段⑤a-1：模拟工厂服务与进程内内核一致 ─────────────────────

#[test]
fn rt_des_30_factory_service_matches_the_in_process_engine() {
    use an_des::{FactoryService, ServiceConfig};
    for (seed, pattern, horizon) in [(1, Pattern::Script, 240), (7, cluster(), 240)] {
        let config = des_config(seed, pattern.clone(), horizon);
        let mut e = Engine::memory().unwrap();
        let sim = e.create_des_simulation(config.clone()).unwrap();
        let mut svc = FactoryService::new(ServiceConfig {
            admin_token: Some("t".into()),
        });
        let created = svc
            .call("simulation_create", serde_json::to_value(&config).unwrap())
            .expect("simulation_create");
        assert_eq!(created["simulation_id"], sim.as_str(), "same simulation id");

        // 同一时刻的可观测状态一致。
        for t in [START + 10, START + 60, START + horizon] {
            e.advance(t).unwrap();
            svc.call(
                "simulation_advance",
                serde_json::json!({"simulation_id": sim, "to_minute": t}),
            )
            .unwrap();
            let remote = svc
                .call(
                    "equipment_status",
                    serde_json::json!({"simulation_id": sim}),
                )
                .unwrap();
            let local = e.des_factory_view(&sim).unwrap();
            assert_eq!(
                remote["machines"], local["machines"],
                "seed {seed} minute {t}: observable machines must match"
            );
        }

        // 事件：服务给出的事件等于内核事件加仿真编号前缀。
        let page = svc
            .call(
                "events_poll",
                serde_json::json!({"simulation_id": sim, "cursor": 0, "limit": 1000}),
            )
            .unwrap();
        let remote: Vec<Value> = page["events"].as_array().unwrap().clone();
        let kernel = run_open_loop(&FactoryConfig::line_b_default(), &config.scenario).unwrap();
        assert_eq!(
            remote.len(),
            kernel.events.len(),
            "seed {seed}: event count"
        );
        for (r, k) in remote.iter().zip(&kernel.events) {
            assert_eq!(r["id"], format!("{sim}:{}", k.id));
            assert_eq!(r["batch"], format!("{sim}:{}", k.batch));
            for f in ["kind", "source", "asset", "customer"] {
                assert_eq!(r[f], serde_json::to_value(k).unwrap()[f], "seed {seed} {f}");
            }
            assert_eq!(r["occurred_at"], k.occurred_at);
            assert_eq!(r["payload"], k.payload, "seed {seed}: payload");
        }

        // 标准答案：管理入口与审核层读到的相同。
        let truth = svc
            .call(
                "simulation_ground_truth",
                serde_json::json!({"simulation_id": sim, "admin_token": "t"}),
            )
            .unwrap();
        assert_eq!(
            truth,
            e.des_review_truth(&sim).unwrap(),
            "seed {seed}: truth"
        );
    }
}
