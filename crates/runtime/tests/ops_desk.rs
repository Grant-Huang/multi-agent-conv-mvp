// 运营桌面 / 运营频道 D0a（后端）验收测试，用例见 docs/testing/CASES.md“运营桌面与运营频道 D0/D1”。
use an_runtime::config::AppConfig;
use an_runtime::mom::ops_desk::*;
use an_runtime::mom::*;
use an_runtime::store::Store;
use std::{path::Path, sync::Arc};

fn store() -> Arc<Store> {
    Arc::new(Store::open(Path::new(":memory:")).unwrap())
}
fn cfg(merge_secs: u64, hourly_cap: u32) -> OpsConfig {
    OpsConfig {
        merge_secs,
        hourly_cap,
    }
}
/// 一个待批准的 S1 案例；batch 不同则是不同案例。事件时间以引擎当前时间为基准（与服务端的场景生成一致），
/// 这样同一个引擎里可以连续建多个案例，每个都能走到待批准。
fn pending_case(e: &mut Engine, batch: &str) -> String {
    let base = e.now().max(497);
    let mut a = EventInput::s1("order", base);
    a.id = format!("{batch}-order");
    a.batch = batch.into();
    let id = e.ingest(a).unwrap();
    for (k, d) in [("vibration", 6), ("quality", 14)] {
        e.advance(base + d).unwrap();
        let mut x = EventInput::s1(k, base + d);
        x.id = format!("{batch}-{k}");
        x.batch = batch.into();
        e.ingest(x).unwrap();
    }
    e.advance(base + 17).unwrap();
    assert_eq!(e.case(&id).unwrap().stage, "WAITING_HUMAN");
    id
}
fn approve(e: &mut Engine, id: &str) {
    let v = e.case(id).unwrap().plan_revision;
    e.approve(id, v, "production", "supervisor").unwrap();
}
fn channel(store: &Store) -> Vec<an_core::MessageView> {
    store.list_message_views(OPS_CHANNEL_ID, None).unwrap()
}
fn ops_cards(store: &Store) -> Vec<an_core::ChatCard> {
    channel(store)
        .into_iter()
        .filter_map(|m| m.card)
        .filter(|c| matches!(c, an_core::ChatCard::OpsCase { .. }))
        .collect()
}

#[test]
fn rt_opsdesk_01_classify_is_deterministic() {
    // 安全、超期是 P0；等待人决定的案例（非纯观测事件）是 P0
    assert_eq!(classify("SafetyBlocked", "BLOCKED"), OpsLevel::P0);
    assert_eq!(classify("SlaBreached", "ASSESS"), OpsLevel::P0);
    assert_eq!(classify("PlanInvalidated", "WAITING_HUMAN"), OpsLevel::P0);
    // 纯观测、过程事件是 P2，不因案例阶段变化
    assert_eq!(classify("ObservationRecorded", "ASSESS"), OpsLevel::P2);
    assert_eq!(
        classify("ObservationRecorded", "WAITING_HUMAN"),
        OpsLevel::P2
    );
    assert_eq!(
        classify("VerificationObservationRecorded", "VERIFYING"),
        OpsLevel::P2
    );
    assert_eq!(classify("ToolStarted", "ASSESS"), OpsLevel::P2);
    // 其余案例状态变化是 P1
    assert_eq!(classify("ActionAccepted", "VERIFYING"), OpsLevel::P1);
    assert_eq!(
        classify("ComplaintVerificationRecorded", "VERIFYING"),
        OpsLevel::P1
    );
    // 同输入同输出
    for _ in 0..3 {
        assert_eq!(classify("SafetyBlocked", "BLOCKED"), OpsLevel::P0);
    }
}

#[test]
fn rt_opsdesk_02_restart_neither_duplicates_nor_loses() {
    let dir = std::env::temp_dir().join(format!("ops-rt02-{}", an_core::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let (mom_db, ops_db) = (dir.join("mom.db"), dir.join("ops.db"));
    let store = store();
    let mut e = Engine::open(&mom_db).unwrap();
    let id = pending_case(&mut e, "RT02");
    let mut p = OpsPublisher::open(&ops_db, store.clone(), cfg(0, 10)).unwrap();
    let r = p.tick(&mut e, 1000).unwrap();
    assert_eq!((r.posted, r.updated), (1, 0));
    assert_eq!(ops_cards(&store).len(), 1);
    drop(p);
    drop(e);
    // 重启：同样的库文件，没有新事件时不重复发
    let mut e = Engine::open(&mom_db).unwrap();
    let mut p = OpsPublisher::open(&ops_db, store.clone(), cfg(0, 10)).unwrap();
    let r = p.tick(&mut e, 2000).unwrap();
    assert_eq!((r.posted, r.updated), (0, 0));
    assert_eq!(channel(&store).len(), 1);
    // 重启期间的新事件不漏：批准后更新原卡，仍只有一条消息
    approve(&mut e, &id);
    let r = p.tick(&mut e, 3000).unwrap();
    assert_eq!((r.posted, r.updated), (0, 1));
    assert_eq!(channel(&store).len(), 1);
    match &ops_cards(&store)[0] {
        an_core::ChatCard::OpsCase { approver, .. } => {
            assert_eq!(approver.as_deref(), Some("supervisor"))
        }
        _ => unreachable!(),
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rt_opsdesk_03_channel_messages_are_data_not_conversation() {
    let store = store();
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT03");
    let mut p = OpsPublisher::memory(store.clone(), cfg(0, 10)).unwrap();
    p.tick(&mut e, 1000).unwrap();
    let conv = store
        .get_conversation(OPS_CHANNEL_ID)
        .unwrap()
        .expect("运营频道会话存在，id 固定");
    assert_eq!(conv.id, OPS_CHANNEL_ID);
    let msgs = channel(&store);
    assert_eq!(msgs.len(), 1);
    let m = &msgs[0];
    assert!(m.message.task_id.is_none(), "不创建任务、不关联任务");
    assert!(
        m.message.content.contains(&id) && m.message.content.contains("模拟"),
        "正文是给人看的说明，并标明模拟"
    );
    match m.card.as_ref().expect("案例卡") {
        an_core::ChatCard::OpsCase {
            case_id,
            provenance,
            level,
            ..
        } => {
            assert_eq!(case_id, &id);
            assert_eq!(provenance, "simulation");
            assert_eq!(level, "P0");
        }
        other => panic!("应是运营案例卡：{other:?}"),
    }
    assert!(
        store.list_tasks(Some(OPS_CHANNEL_ID)).unwrap().is_empty(),
        "不创建任务"
    );
}

#[test]
fn rt_opsdesk_04_switch_defaults_off() {
    let (cfg, unknown) =
        AppConfig::parse("[llm]\nbase_url=\"http://127.0.0.1:1/v1\"\nmodel=\"m\"\n").unwrap();
    assert!(unknown.is_empty());
    assert!(!cfg.mom.chat_bridge, "未配置 [mom] 时开关关闭");
    let (cfg, unknown) = AppConfig::parse(
        "[llm]\nbase_url=\"http://127.0.0.1:1/v1\"\nmodel=\"m\"\n[mom]\nchat_bridge=true\n",
    )
    .unwrap();
    assert!(unknown.is_empty());
    assert!(cfg.mom.chat_bridge);
    assert_eq!(cfg.mom.ops_merge_secs, 60, "合并窗口默认 60 秒");
    assert_eq!(cfg.mom.ops_hourly_cap, 10, "每小时上限默认 10 张");
    let (cfg, _) = AppConfig::parse("[llm]\nbase_url=\"http://127.0.0.1:1/v1\"\nmodel=\"m\"\n[mom]\nchat_bridge=true\nops_merge_secs=1\nops_hourly_cap=3\n").unwrap();
    assert_eq!((cfg.mom.ops_merge_secs, cfg.mom.ops_hourly_cap), (1, 3));
}

#[test]
fn rt_opsdesk_05_skips_are_explained_and_cap_summarizes() {
    // 合并窗口：窗口内的更新记下原因，窗口后才更新原卡
    let store1 = store();
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT05A");
    let mut p = OpsPublisher::memory(store1.clone(), cfg(60, 10)).unwrap();
    assert_eq!(p.tick(&mut e, 1000).unwrap().posted, 1);
    approve(&mut e, &id);
    let r = p.tick(&mut e, 1010).unwrap();
    assert_eq!((r.updated, r.skipped), (0, 1));
    let log = p.skip_log().unwrap();
    assert!(
        log.iter()
            .any(|s| s.case_id == id && s.reason == "merge_window"),
        "{log:?}"
    );
    assert_eq!(p.tick(&mut e, 1061).unwrap().updated, 1);
    assert_eq!(channel(&store1).len(), 1);

    // 每小时上限：超出的案例并入一张汇总卡，并记录原因
    let store2 = store();
    let mut e = Engine::memory().unwrap();
    let ids: Vec<String> = (0..5)
        .map(|i| pending_case(&mut e, &format!("RT05B{i}")))
        .collect();
    let mut p = OpsPublisher::memory(store2.clone(), cfg(0, 2)).unwrap();
    let r = p.tick(&mut e, 5000).unwrap();
    assert_eq!((r.posted, r.summarized), (2, 3));
    assert_eq!(ops_cards(&store2).len(), 2);
    let summary: Vec<_> = channel(&store2)
        .into_iter()
        .filter_map(|m| match m.card {
            Some(an_core::ChatCard::OpsSummary { count, case_ids }) => Some((count, case_ids)),
            _ => None,
        })
        .collect();
    assert_eq!(summary.len(), 1, "汇总卡只有一张");
    assert_eq!(summary[0].0, 3);
    for id in &ids {
        let carded = ops_cards(&store2)
            .iter()
            .any(|c| matches!(c, an_core::ChatCard::OpsCase { case_id, .. } if case_id == id));
        assert!(
            carded || summary[0].1.contains(id),
            "{id} 既没有卡也不在汇总里：被静默丢弃"
        );
    }
    let log = p.skip_log().unwrap();
    assert_eq!(
        log.iter().filter(|s| s.reason == "hourly_cap").count(),
        3,
        "{log:?}"
    );
}

#[test]
fn rt_opsdesk_08_ensure_conversation_uses_fixed_id_and_is_idempotent() {
    let store = store();
    let a = store
        .ensure_conversation("ops-channel", "运营频道")
        .unwrap();
    assert_eq!(a.id, "ops-channel");
    store
        .add_message("ops-channel", "assistant", "x", None)
        .unwrap();
    let b = store
        .ensure_conversation("ops-channel", "别的标题")
        .unwrap();
    assert_eq!(b.id, "ops-channel");
    assert_eq!(b.title, "运营频道", "已存在时不改标题");
    assert_eq!(b.created_at, a.created_at);
    assert_eq!(
        store
            .list_conversations()
            .unwrap()
            .iter()
            .filter(|c| c.id == "ops-channel")
            .count(),
        1
    );
    assert_eq!(
        store.list_message_views("ops-channel", None).unwrap().len(),
        1,
        "不清空已有消息"
    );
}

#[test]
fn rt_opsdesk_09_unread_counts_new_messages_until_read() {
    let dir = std::env::temp_dir().join(format!("ops-rt09-{}", an_core::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let ops_db = dir.join("ops.db");
    let store = store();
    let mut e = Engine::memory().unwrap();
    let a = pending_case(&mut e, "RT09A");
    let mut p = OpsPublisher::open(&ops_db, store.clone(), cfg(0, 10)).unwrap();
    assert_eq!(p.unread().unwrap(), 0, "还没有消息");
    p.tick(&mut e, 1000).unwrap();
    assert_eq!(p.unread().unwrap(), 1);
    // 就地更新原卡不增加未读（批准要在案例证据过期前做，所以紧接着发完卡就批准）
    approve(&mut e, &a);
    assert_eq!(p.tick(&mut e, 1001).unwrap().updated, 1);
    assert_eq!(p.unread().unwrap(), 1);
    pending_case(&mut e, "RT09B");
    p.tick(&mut e, 1002).unwrap();
    assert_eq!(p.unread().unwrap(), 2);
    p.mark_read().unwrap();
    assert_eq!(p.unread().unwrap(), 0);
    // 新卡增加未读；重启后已读位置仍在
    pending_case(&mut e, "RT09C");
    p.tick(&mut e, 1003).unwrap();
    assert_eq!(p.unread().unwrap(), 1);
    drop(p);
    let p = OpsPublisher::open(&ops_db, store, cfg(0, 10)).unwrap();
    assert_eq!(p.unread().unwrap(), 1);
    let _ = std::fs::remove_dir_all(dir);
}
