//! RT-ADXR-08（M4，Issue #361）：旧库的 desktop_assistant_read_ids 没有 user_id 列时，
//! 打开自动迁移为 (user_id, id) 主键、旧行并入 local 单用户（与 5.2a 作用域契约一致），
//! 可重复打开，已读状态不丢。

use an_runtime::store::Store;

#[test]
fn rt_adxr_08_old_read_table_migrates_to_user_scoped() {
    let dir = std::env::temp_dir().join(an_core::new_id("adxr-migrate"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.db");
    {
        // 按旧结构建表并写一条已读（旧库无用户维度）
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE desktop_assistant_read_ids (
                id TEXT PRIMARY KEY,
                updated_at TEXT NOT NULL DEFAULT ''
            );
            INSERT INTO desktop_assistant_read_ids VALUES ('asst-msg-1', '2026-01-01T00:00:00Z');",
        )
        .unwrap();
    }
    let has_user_column = || {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('desktop_assistant_read_ids') WHERE name = 'user_id'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    };
    assert_eq!(has_user_column(), 0, "前提：旧库没有 user_id 列");

    for _ in 0..2 {
        let store = Store::open(&path).expect("旧库应能打开（可重复打开）");
        let local = store.list_desktop_assistant_read_ids("local").unwrap();
        assert_eq!(local, ["asst-msg-1"], "旧行应并入 local 用户");
        assert!(
            store
                .list_desktop_assistant_read_ids("alice")
                .unwrap()
                .is_empty(),
            "迁移后的旧行不属于其他用户"
        );
        let msgs = store.list_desktop_assistant_messages("local").unwrap();
        let m1 = msgs
            .iter()
            .find(|m| m.id == "asst-msg-1")
            .expect("asst-msg-1 应存在");
        assert!(m1.read, "local 应看到迁移保住的已读态");
        drop(store);
        assert_eq!(has_user_column(), 1, "打开后应完成迁移");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
