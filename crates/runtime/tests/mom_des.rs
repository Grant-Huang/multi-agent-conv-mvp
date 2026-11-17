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
