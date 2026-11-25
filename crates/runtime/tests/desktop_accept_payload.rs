//! RT-ADACCEPT-05～07：完整载荷、并发和同库重放。
use std::path::Path;
use std::sync::{Arc, Barrier};

use an_runtime::store::{DesktopAcceptOutcome, Store};

#[test]
fn rt_adaccept_05_changed_payload_conflicts_without_writes() {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let a = store.create_conversation("A").unwrap().id;
    let b = store.create_conversation("B").unwrap().id;
    let first = store
        .accept_desktop_request(&a, "op", "查询 SMT-03", "continue", None)
        .unwrap();
    let DesktopAcceptOutcome::Created(original) = first else {
        panic!("首次应创建");
    };
    for (conv, text, scope, task) in [
        (b.as_str(), "查询 SMT-03", "continue", None),
        (a.as_str(), "查询 SMT-03", "new", None),
        (a.as_str(), "查询 SMT-03", "continue", Some("other-task")),
        (a.as_str(), "不同正文", "continue", None),
    ] {
        let result = store
            .accept_desktop_request(conv, "op", text, scope, task)
            .unwrap();
        assert!(
            matches!(result, DesktopAcceptOutcome::Conflict { .. }),
            "不同载荷必须冲突，得到 {result:?}"
        );
    }
    assert_eq!(store.list_messages(&a, None).unwrap().len(), 1);
    assert!(store.list_messages(&b, None).unwrap().is_empty());
    let stored = store
        .get_desktop_request(&original.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.message_id, original.message_id);
    assert_eq!(stored.conversation_id, a);
    assert_eq!(stored.scope, "continue");
    assert_eq!(stored.task_id, None);
    assert_eq!(stored.dispatch_count, 1);
}

#[test]
fn rt_adaccept_06_concurrent_key_has_one_payload_owner() {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let a = store.create_conversation("A").unwrap().id;
    let b = store.create_conversation("B").unwrap().id;
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            let conv = if i % 2 == 0 { a.clone() } else { b.clone() };
            std::thread::spawn(move || {
                barrier.wait();
                let outcome = store
                    .accept_desktop_request(&conv, "concurrent", "同一正文", "continue", None)
                    .unwrap();
                (conv, outcome)
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    let created: Vec<_> = results
        .iter()
        .filter_map(|(_, o)| match o {
            DesktopAcceptOutcome::Created(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 1);
    let winner = created[0];
    let mut replays = 0;
    let mut conflicts = 0;
    for (conv, outcome) in &results {
        if conv == &winner.conversation_id {
            match outcome {
                DesktopAcceptOutcome::Created(_) => {}
                DesktopAcceptOutcome::Replay(r) => {
                    assert_eq!(r.request_id, winner.request_id);
                    assert_eq!(r.message_id, winner.message_id);
                    assert_eq!(r.conversation_id, winner.conversation_id);
                    replays += 1;
                }
                _ => panic!("相同载荷应重放"),
            }
        } else {
            assert!(matches!(outcome, DesktopAcceptOutcome::Conflict { .. }));
            conflicts += 1;
        }
    }
    assert_eq!(replays, 3);
    assert_eq!(conflicts, 4);
    assert_eq!(
        store.list_messages(&a, None).unwrap().len() + store.list_messages(&b, None).unwrap().len(),
        1
    );
}

#[test]
fn rt_adaccept_07_same_database_replays_committed_payload() {
    let dir = std::env::temp_dir().join(an_core::new_id("accept-payload"));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("an.db");
    let (original, a, b) = {
        let store = Store::open(&db).unwrap();
        let a = store.create_conversation("A").unwrap().id;
        let b = store.create_conversation("B").unwrap().id;
        let DesktopAcceptOutcome::Created(r) = store
            .accept_desktop_request(&a, "restart", "  正文  ", "new", None)
            .unwrap()
        else {
            panic!("首次应创建");
        };
        (r, a, b)
    };
    {
        let store = Store::open(&db).unwrap();
        let DesktopAcceptOutcome::Replay(r) = store
            .accept_desktop_request(&a, "restart", "正文", "new", None)
            .unwrap()
        else {
            panic!("相同载荷应重放");
        };
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        assert!(matches!(
            store
                .accept_desktop_request(&b, "restart", "正文", "new", None)
                .unwrap(),
            DesktopAcceptOutcome::Conflict { .. }
        ));
        assert_eq!(store.list_messages(&a, None).unwrap().len(), 1);
        assert!(store.list_messages(&b, None).unwrap().is_empty());
    }
    std::fs::remove_dir_all(dir).unwrap();
}
