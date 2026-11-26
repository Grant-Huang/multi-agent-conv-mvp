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
fn rt_opsdesk_05_repeated_updates_count_a_summarized_case_once() {
    let store = store();
    let mut engine = Engine::memory().unwrap();
    let mut publisher = OpsPublisher::memory(store.clone(), cfg(0, 1)).unwrap();

    pending_case(&mut engine, "RT05-FIRST");
    assert_eq!(publisher.tick(&mut engine, 5000).unwrap().posted, 1);
    let repeated = pending_case(&mut engine, "RT05-REPEATED");
    assert_eq!(publisher.tick(&mut engine, 5001).unwrap().summarized, 1);

    approve(&mut engine, &repeated);
    publisher.tick(&mut engine, 5002).unwrap();
    let another = pending_case(&mut engine, "RT05-ANOTHER");
    publisher.tick(&mut engine, 5003).unwrap();

    let summaries: Vec<_> = channel(&store)
        .into_iter()
        .filter_map(|message| match message.card {
            Some(an_core::ChatCard::OpsSummary { count, case_ids }) => Some((count, case_ids)),
            _ => None,
        })
        .collect();
    assert_eq!(summaries, vec![(2, vec![repeated, another])]);
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
fn rt_opsdesk_06_open_in_channel_is_idempotent() {
    let store = store();
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT06");
    let p = OpsPublisher::memory(store.clone(), cfg(0, 10)).unwrap();
    // 不调用 tick：用户主动“在频道中处理”也能发卡
    let first = p.open_case(&e, &id, 1000).unwrap();
    let second = p.open_case(&e, &id, 1001).unwrap();
    assert_eq!(first, second, "重复调用返回同一消息 id");
    assert!(!first.is_empty(), "消息 id 非空");
    let cards: Vec<_> = ops_cards(&store)
        .into_iter()
        .filter(|c| matches!(c, an_core::ChatCard::OpsCase { case_id, .. } if case_id == &id))
        .collect();
    assert_eq!(cards.len(), 1, "频道里该案例只有一张卡");
    let before = channel(&store).len();
    // 之后 tick 只就地更新，不新增消息
    let mut p = p;
    p.tick(&mut e, 1002).unwrap();
    assert_eq!(channel(&store).len(), before);
    let err = p
        .open_case(&e, "no-such-case", 1003)
        .expect_err("案例不存在应报错");
    assert!(
        err.to_string().starts_with("E_CASE_NOT_FOUND"),
        "错误应以 E_CASE_NOT_FOUND 开头：{err}"
    );
}

fn blocked_case(e: &mut Engine, batch: &str) -> String {
    let input = EventInput {
        id: format!("{batch}-trip-0"),
        scenario: "s1".into(),
        kind: "vibration".into(),
        source: "EAM".into(),
        occurred_at: e.now().max(900),
        batch: batch.into(),
        customer: "A".into(),
        asset: "CNC-XX".into(),
        payload: serde_json::json!({"line":"Line-B","safety_trip":true}),
    };
    let id = e.ingest(input).unwrap();
    assert_eq!(e.case(&id).unwrap().stage, "BLOCKED");
    id
}

fn closed_case(e: &mut Engine, batch: &str) -> String {
    let id = pending_case(e, batch);
    approve(e, &id);
    e.execute(&id, "production").unwrap();
    e.advance(e.now() + 5).unwrap();
    e.close(&id).unwrap();
    assert_eq!(e.case(&id).unwrap().stage, "CLOSED");
    id
}

#[test]
fn rt_opsdesk_07_desk_view_matches_engine() {
    use an_runtime::mom::ops_view::desk_view;

    let empty = Engine::memory().unwrap();
    let v = desk_view(&empty).unwrap();
    assert_eq!(v.state, "no_data");
    assert!(v.rows.is_empty());
    assert_eq!(v.mode, "simulation");

    let mut e = Engine::memory().unwrap();
    let pending = pending_case(&mut e, "RT07P");
    let blocked = blocked_case(&mut e, "RT07B");
    let closed = closed_case(&mut e, "RT07C");
    let v = desk_view(&e).unwrap();
    assert_eq!(v.state, "ok");
    assert_eq!(v.strip.pending, 1);
    assert_eq!(v.strip.blocked, 1);
    assert_eq!(v.strip.completed, 1);
    assert!(
        v.rows.len() >= 3,
        "应包含待批准、受阻、已关闭：{}",
        v.rows.len()
    );
    assert_eq!(v.rows[0].case_id, pending);
    assert_eq!(v.rows[0].group, "pending");
    assert!(v.rows[0].stage_label.contains("待批准"));
    assert_eq!(v.rows[1].case_id, blocked);
    assert_eq!(v.rows[1].group, "blocked");
    assert!(
        v.rows[1].stage_label.contains("受阻"),
        "{}",
        v.rows[1].stage_label
    );
    assert_eq!(v.rows.last().unwrap().case_id, closed);
    assert_eq!(v.rows.last().unwrap().group, "completed");
    for row in &v.rows {
        assert_eq!(row.provenance, "simulation");
        assert!(
            e.case_ids().contains(&row.case_id),
            "桌面行 {} 不在引擎里",
            row.case_id
        );
    }
    // 同组排序稳定：再调一次结果一致
    assert_eq!(
        desk_view(&e)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.case_id.clone())
            .collect::<Vec<_>>(),
        v.rows.iter().map(|r| r.case_id.clone()).collect::<Vec<_>>()
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

// INTENT-027（Issue #340，决定 2026-11-26-ops-approve-auto-execute）：批准即执行。
fn approval_expiry_of(e: &Engine, id: &str, scope: &str) -> u64 {
    e.case(id)
        .unwrap()
        .approvals
        .get(scope)
        .map(|a| a.expires_at)
        .expect("approval recorded")
}
fn card_summary(store: &Store) -> String {
    match &ops_cards(store)[0] {
        an_core::ChatCard::OpsCase { summary, .. } => summary.clone(),
        other => panic!("expected ops case card, got {other:?}"),
    }
}

#[test]
fn rt_opsdesk_12_approve_and_execute_dispatches_or_blocks_with_approval_kept() {
    // 正常案例：批准即执行，动作派发、无受阻提示
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT12A");
    let revision = e.case(&id).unwrap().plan_revision;
    let outcome = e
        .approve_and_execute(&id, revision, "production", "supervisor")
        .unwrap();
    assert_eq!(outcome, ApproveOutcome::Executed);
    let c = e.case(&id).unwrap();
    assert_eq!(c.stage, "VERIFYING");
    assert!(c
        .actions
        .contains_key(&format!("{id}:{revision}:production:mro.transfer")));
    assert!(c.execution_block.is_none());
    // 幂等：同批准再点只是查询
    let outcome = e
        .approve_and_execute(&id, revision, "production", "supervisor")
        .unwrap();
    assert_eq!(outcome, ApproveOutcome::Already);
    // 错误方案版本：整体失败，什么都不留
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT12B");
    assert!(e
        .approve_and_execute(&id, 99, "production", "supervisor")
        .is_err());
    assert!(e.case(&id).unwrap().approvals.is_empty());
    // 受阻案例：批准保留、无动作、受阻原因入卡、事件发出
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT12C");
    e.change_world(&id, "resources_available", serde_json::json!(false))
        .unwrap();
    let revision = e.case(&id).unwrap().plan_revision;
    let outcome = e
        .approve_and_execute(&id, revision, "production", "supervisor")
        .unwrap();
    let ApproveOutcome::Blocked { reason } = outcome else {
        panic!("expected Blocked, got {outcome:?}")
    };
    assert!(reason.contains("resources unavailable"), "reason: {reason}");
    let c = e.case(&id).unwrap();
    assert_eq!(c.stage, "WAITING_HUMAN");
    assert!(c.actions.is_empty());
    assert_eq!(
        c.execution_block.as_ref().map(|b| b.reason.as_str()),
        Some(reason.as_str())
    );
    let events: Vec<_> = e
        .pending_events(0, 100)
        .unwrap()
        .into_iter()
        .filter(|ev| ev.kind == "ExecutionBlocked")
        .collect();
    assert_eq!(events.len(), 1);
    // 到期前重试：仍受阻，批准不重复登记（有效期不变）
    let expiry_before = approval_expiry_of(&e, &id, "production");
    let outcome = e
        .approve_and_execute(&id, revision, "production", "supervisor")
        .unwrap();
    assert!(matches!(outcome, ApproveOutcome::Blocked { .. }));
    assert_eq!(approval_expiry_of(&e, &id, "production"), expiry_before);
}

#[test]
fn rt_opsdesk_13_approval_expiry_emits_once_and_card_prompts_reapproval() {
    let dir = std::env::temp_dir().join(format!("ops-rt13-{}", an_core::new_id("t")));
    std::fs::create_dir_all(&dir).unwrap();
    let store = store();
    let mut e = Engine::memory().unwrap();
    let id = pending_case(&mut e, "RT13");
    e.change_world(&id, "resources_available", serde_json::json!(false))
        .unwrap();
    let revision = e.case(&id).unwrap().plan_revision;
    assert!(matches!(
        e.approve_and_execute(&id, revision, "production", "supervisor")
            .unwrap(),
        ApproveOutcome::Blocked { .. }
    ));
    let expiry = approval_expiry_of(&e, &id, "production");
    let mut p = OpsPublisher::memory(store.clone(), cfg(0, 10)).unwrap();
    p.tick(&mut e, 1000).unwrap();
    assert!(
        !card_summary(&store).contains("批准已过期"),
        "未到期不应提示：{}",
        card_summary(&store)
    );
    // 越过有效期：发一次 ApprovalExpired，卡片提示重新批准
    e.advance(expiry + 1).unwrap();
    e.advance(expiry + 2).unwrap();
    let expired_events: Vec<_> = e
        .pending_events(0, 500)
        .unwrap()
        .into_iter()
        .filter(|ev| ev.kind == "ApprovalExpired")
        .collect();
    assert_eq!(expired_events.len(), 1, "过期提醒只发一次");
    p.tick(&mut e, 1001).unwrap();
    let summary = card_summary(&store);
    assert!(summary.contains("批准已过期"), "summary: {summary}");
    assert!(summary.contains("需重新批准"), "summary: {summary}");
    // 批准记录保留（审计），approver 仍在卡上
    assert_eq!(
        e.case(&id).unwrap().approvals["production"].approver,
        "supervisor"
    );
    // 恢复资源 + 刷新方案（现行补救路径）后重新批准：即执行成功，新有效期刷新
    e.change_world(&id, "resources_available", serde_json::json!(true))
        .unwrap();
    e.refresh_plan(&id).unwrap();
    e.advance(e.now() + 3).unwrap();
    let revision = e.case(&id).unwrap().plan_revision;
    assert_eq!(e.case(&id).unwrap().stage, "WAITING_HUMAN");
    let outcome = e
        .approve_and_execute(&id, revision, "production", "supervisor")
        .unwrap();
    assert_eq!(outcome, ApproveOutcome::Executed);
    assert!(approval_expiry_of(&e, &id, "production") > expiry);
    assert!(e.case(&id).unwrap().execution_block.is_none());
    p.tick(&mut e, 1002).unwrap();
    assert!(card_summary(&store).contains("核验中") || e.case(&id).unwrap().stage == "VERIFYING");
    let _ = std::fs::remove_dir_all(dir);
}
