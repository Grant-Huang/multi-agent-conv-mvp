//! RT-ADM-01..07：agent 准入与持续信任的协议核心（用例见 docs/testing/CASES.md）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use an_core::Risk;
use an_runtime::admission::*;
use an_runtime::store::Store;

fn ledger_with(cfg: AdmissionConfig) -> AdmissionLedger {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    AdmissionLedger::open(store, cfg).unwrap()
}

fn ledger() -> AdmissionLedger {
    ledger_with(AdmissionConfig::default())
}

/// 一份自洽的 Card（不会产生澄清问题）
fn claim(id: &str, level: TrustLevel) -> AgentClaim {
    let mut caps = BTreeMap::new();
    for d in CAPABILITY_DIMS {
        caps.insert(d.to_string(), false);
    }
    caps.insert("read_context".into(), true);
    AgentClaim {
        agent_id: id.into(),
        name: format!("agent {id}"),
        description: "质量分析".into(),
        capabilities: caps,
        requested_level: level,
        max_subtask_depth: None,
        working_language: Some("zh".into()),
    }
}

fn contract() -> Contract {
    Contract {
        role: "executor".into(),
        working_language: "zh".into(),
        context_scope: vec!["quality".into()],
        allowed_tools: vec![],
    }
}

fn scores(c: f64, v: f64, t: f64) -> Scores {
    Scores {
        completeness: c,
        conversation: v,
        test: t,
    }
}

/// 把 agent 推进到激活（需要审批时先审批），返回 epoch
fn admit(l: &AdmissionLedger, id: &str, level: TrustLevel, s: Scores) -> u64 {
    let q = l.submit(claim(id, level)).unwrap();
    l.clarify(id, q.iter().map(|_| "已说明".to_string()).collect())
        .unwrap();
    l.score(id, s).unwrap();
    l.contract(id, contract()).unwrap();
    if l.get(id).unwrap().granted == Some(TrustLevel::Trusted) {
        l.approve(id, "supervisor").unwrap();
    }
    l.activate(id).unwrap()
}

#[test]
fn rt_adm_01_protocol_order_and_clarifications() {
    let l = ledger();
    // 跳步：未澄清就打分
    l.submit(claim("a1", TrustLevel::Member)).unwrap();
    let e = l.score("a1", scores(1.0, 1.0, 1.0)).unwrap_err();
    assert!(
        format!("{e:#}").contains(an_core::codes::ADMISSION_STATE),
        "{e:#}"
    );
    // 未签契约就激活
    assert!(l.activate("a1").is_err());
    // 自相矛盾的 Card 产生澄清问题
    let mut c = claim("a2", TrustLevel::Member);
    c.capabilities.insert("spawn_subtasks".into(), true);
    c.working_language = None;
    let q = l.submit(c).unwrap();
    assert!(q.iter().any(|x| x.contains("子任务")), "{q:?}");
    assert!(q.iter().any(|x| x.contains("工作语言")), "{q:?}");
    // 回答数不符
    assert!(l.clarify("a2", vec!["只答一条".into()]).is_err());
    assert!(l
        .clarify("a2", vec!["深度 2".into(), "中文".into()])
        .is_ok());
    // 非法 agent_id、未知能力维度、重复提交
    assert!(l.submit(claim("bad id!", TrustLevel::Guest)).is_err());
    let mut c = claim("a3", TrustLevel::Guest);
    c.capabilities.insert("launch_missiles".into(), true);
    assert!(l.submit(c).is_err());
    assert!(l.submit(claim("a1", TrustLevel::Guest)).is_err());
    // 完整流程可以走通
    assert_eq!(
        admit(&l, "a4", TrustLevel::Member, scores(1.0, 1.0, 0.6)),
        1
    );
    assert_eq!(l.get("a4").unwrap().stage, Stage::Active);
}

#[test]
fn rt_adm_02_weighted_score_and_level() {
    let l = ledger();
    for (id, req, s, total, level) in [
        (
            "s1",
            TrustLevel::Trusted,
            scores(1.0, 1.0, 1.0),
            100.0,
            TrustLevel::Trusted,
        ),
        (
            "s2",
            TrustLevel::Trusted,
            scores(1.0, 0.5, 0.6),
            70.0,
            TrustLevel::Member,
        ),
        (
            "s3",
            TrustLevel::Trusted,
            scores(0.5, 0.5, 0.5),
            50.0,
            TrustLevel::Guest,
        ),
        // 推荐等级不超过申请等级
        (
            "s4",
            TrustLevel::Member,
            scores(1.0, 1.0, 1.0),
            100.0,
            TrustLevel::Member,
        ),
    ] {
        let q = l.submit(claim(id, req)).unwrap();
        l.clarify(id, q.iter().map(|_| "x".to_string()).collect())
            .unwrap();
        let (t, lv) = l.score(id, s).unwrap();
        assert!((t - total).abs() < 1e-9, "{id}: {t}");
        assert_eq!(lv, level, "{id}");
    }
    // 权重可配置：只看测试分
    let cfg = AdmissionConfig {
        weights: Weights {
            completeness: 0.0,
            conversation: 0.0,
            test: 1.0,
        },
        ..AdmissionConfig::default()
    };
    let l = ledger_with(cfg);
    l.submit(claim("w", TrustLevel::Trusted)).unwrap();
    l.clarify("w", vec![]).unwrap();
    let (t, _) = l.score("w", scores(0.0, 0.0, 0.9)).unwrap();
    assert!((t - 90.0).abs() < 1e-9);
}

#[test]
fn rt_adm_03_human_approval_gate() {
    let l = ledger();
    // Trusted 未审批不能激活
    l.submit(claim("t", TrustLevel::Trusted)).unwrap();
    l.clarify("t", vec![]).unwrap();
    l.score("t", scores(1.0, 1.0, 1.0)).unwrap();
    l.contract("t", contract()).unwrap();
    assert!(l.activate("t").is_err());
    l.approve("t", "supervisor").unwrap();
    assert_eq!(l.activate("t").unwrap(), 1);
    assert_eq!(
        l.get("t").unwrap().approved_by.as_deref(),
        Some("supervisor")
    );
    // Guest、Member 默认无需审批
    assert_eq!(admit(&l, "g", TrustLevel::Guest, scores(0.1, 0.1, 0.1)), 1);
    assert_eq!(admit(&l, "m", TrustLevel::Member, scores(1.0, 1.0, 0.6)), 1);
    // 配置要求 Member 审批
    let l = ledger_with(AdmissionConfig {
        member_requires_approval: true,
        ..AdmissionConfig::default()
    });
    l.submit(claim("m2", TrustLevel::Member)).unwrap();
    l.clarify("m2", vec![]).unwrap();
    l.score("m2", scores(1.0, 1.0, 0.6)).unwrap();
    l.contract("m2", contract()).unwrap();
    assert!(l.activate("m2").is_err());
}

#[test]
fn rt_adm_04_deterministic_sampling() {
    let l = ledger();
    admit(&l, "p", TrustLevel::Member, scores(1.0, 1.0, 0.6));
    let ids: Vec<String> = (0..10_000).map(|i| format!("task-{i}")).collect();
    let hits: Vec<bool> = ids.iter().map(|t| l.should_sample("p", t)).collect();
    let again: Vec<bool> = ids.iter().map(|t| l.should_sample("p", t)).collect();
    assert_eq!(hits, again, "同一输入结果必须相同");
    let rate = hits.iter().filter(|h| **h).count() as f64 / ids.len() as f64;
    assert!((0.17..=0.23).contains(&rate), "rate {rate}");
    let other = ledger_with(AdmissionConfig {
        seed: 7,
        ..AdmissionConfig::default()
    });
    admit(&other, "p", TrustLevel::Member, scores(1.0, 1.0, 0.6));
    let hits2: Vec<bool> = ids.iter().map(|t| other.should_sample("p", t)).collect();
    assert_ne!(hits, hits2, "换 seed 抽中集合应不同");
}

#[test]
fn rt_adm_05_probation_warnings_and_downgrade() {
    let l = ledger();
    let epoch = admit(&l, "d", TrustLevel::Member, scores(1.0, 1.0, 0.6));
    assert!(l.permits("d", Risk::Medium));
    // 不合格、合格、不合格：连续计数被清零，不记警告
    assert_eq!(l.record_check("d", false).unwrap(), CheckOutcome::Recorded);
    assert_eq!(l.record_check("d", true).unwrap(), CheckOutcome::Recorded);
    assert_eq!(l.record_check("d", false).unwrap(), CheckOutcome::Recorded);
    assert_eq!(l.get("d").unwrap().probation.warnings, 0);
    // 连续两次 → 第 1 次警告
    assert_eq!(l.record_check("d", false).unwrap(), CheckOutcome::Warning);
    assert_eq!(l.get("d").unwrap().probation.warnings, 1);
    for _ in 0..2 {
        l.record_check("d", false).unwrap();
    }
    l.record_check("d", false).unwrap();
    assert_eq!(l.get("d").unwrap().probation.warnings, 2);
    // 此时连续计数为 1，再不合格一次 → 第 3 次警告 → 降级
    assert_eq!(
        l.record_check("d", false).unwrap(),
        CheckOutcome::Downgraded
    );
    let r = l.get("d").unwrap();
    assert_eq!(r.granted, Some(TrustLevel::Guest));
    assert!(r.probation.pending_review);
    assert_eq!(r.epoch, epoch + 1);
    assert!(!l.credential_valid("d", epoch), "旧凭证必须失效");
    assert!(l.credential_valid("d", epoch + 1));
    assert!(!l.permits("d", Risk::Medium));
    assert!(l.permits("d", Risk::Low));
}

#[test]
fn rt_adm_06_revocation() {
    let l = ledger();
    let epoch = admit(&l, "r", TrustLevel::Member, scores(1.0, 1.0, 0.6));
    assert!(l.credential_valid("r", epoch));
    let new_epoch = l.revoke("r", "越权调用").unwrap();
    assert_eq!(new_epoch, epoch + 1);
    let rec = l.get("r").unwrap();
    assert_eq!(rec.stage, Stage::Revoked);
    assert!(!l.credential_valid("r", epoch));
    assert!(
        !l.credential_valid("r", new_epoch),
        "被撤权后任何凭证都无效"
    );
    for risk in [Risk::Low, Risk::Medium, Risk::High] {
        assert!(!l.permits("r", risk));
    }
    assert!(l.record_check("r", true).is_err());
    // 预准入（配置里写的 agent）为 Trusted，也可以被撤权
    let p = l.preadmit("general").unwrap();
    assert_eq!(p.granted, Some(TrustLevel::Trusted));
    assert_eq!(p.source, "config");
    assert!(l.permits("general", Risk::High));
    // 预准入幂等
    assert_eq!(l.preadmit("general").unwrap().epoch, p.epoch);
    l.revoke("general", "测试").unwrap();
    assert!(!l.permits("general", Risk::Low));
    // 未知 agent 一律不允许
    assert!(!l.permits("nobody", Risk::Low));
    assert!(!l.credential_valid("nobody", 1));
}

#[test]
fn rt_adm_07_persistence_and_audit_order() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-adm-test"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("adm.db");
    let before = {
        let store = Arc::new(Store::open(&db).unwrap());
        let l = AdmissionLedger::open(store, AdmissionConfig::default()).unwrap();
        admit(&l, "x", TrustLevel::Member, scores(1.0, 1.0, 0.6));
        l.record_check("x", false).unwrap();
        l.revoke("x", "测试").unwrap();
        l.preadmit("general").unwrap();
        (l.get("x").unwrap(), l.get("general").unwrap())
    };
    let store = Arc::new(Store::open(&db).unwrap());
    let l = AdmissionLedger::open(store, AdmissionConfig::default()).unwrap();
    assert_eq!(l.get("x").unwrap(), before.0);
    assert_eq!(l.get("general").unwrap(), before.1);
    let kinds: Vec<String> = l
        .events("x", 100)
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "admission.submitted",
            "admission.clarified",
            "admission.scored",
            "admission.contracted",
            "admission.activated",
            "admission.check",
            "admission.revoked"
        ]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// RT-ADM-08：旧库（没有准入表）打开时自动新建两张表，旧数据不受影响，重复打开幂等
#[test]
fn rt_adm_08_old_database_gets_admission_tables() {
    let dir = std::env::temp_dir().join(an_core::new_id("an-adm-mig"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("old.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE nodes (node_id TEXT PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL,
                token TEXT NOT NULL, cards_json TEXT NOT NULL, paired_at TEXT NOT NULL);
             INSERT INTO nodes VALUES ('n1','mac','10.0.0.2:1','tok','[]','2026-01-01');",
        )
        .unwrap();
    }
    let store = Store::open(&db).unwrap();
    let tables: Vec<String> = {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let mut st = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' \
                 AND name IN ('agent_admission','admission_events') ORDER BY name",
            )
            .unwrap();
        let rows = st.query_map([], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    assert_eq!(tables, vec!["admission_events", "agent_admission"]);
    assert_eq!(store.list_nodes().unwrap().len(), 1, "旧数据必须保留");
    store
        .save_admission(
            "a",
            r#"{"agent_id":"a"}"#,
            &[("admission.submitted".into(), "{}".into())],
        )
        .unwrap();
    assert_eq!(store.list_admissions().unwrap().len(), 1);
    assert_eq!(store.admission_events("a", 10).unwrap().len(), 1);
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.list_admissions().unwrap().len(), 1);
    assert_eq!(store.list_nodes().unwrap().len(), 1);
    drop(store);
    std::fs::remove_dir_all(&dir).unwrap();
}
