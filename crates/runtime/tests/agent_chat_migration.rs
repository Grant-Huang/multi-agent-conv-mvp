//! RT-AGCHAT-08（定义见 docs/testing/CASES.md「对话里管理助手」）：
//! 旧数据库的 messages 表没有 card_json 列时，打开数据库自动补列、可重复打开，旧消息照常可读。

use an_runtime::store::Store;

#[test]
fn rt_agchat_08_old_db_gets_card_column() {
    let dir = std::env::temp_dir().join(an_core::new_id("agchat-migrate"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.db");
    {
        // 按旧结构建表并写一条消息
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversations (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE messages (id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, role TEXT NOT NULL,
                                    content TEXT NOT NULL, task_id TEXT, created_at TEXT NOT NULL);
             INSERT INTO conversations VALUES ('c1', '旧会话', '2026-01-01T00:00:00Z');
             INSERT INTO messages VALUES ('m1', 'c1', 'user', '旧消息', NULL, '2026-01-01T00:00:00Z');",
        )
        .unwrap();
    }
    let has_card_column = || {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'card_json'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
    };
    assert_eq!(has_card_column(), 0, "前提：旧库没有 card_json 列");

    for _ in 0..2 {
        let store = Store::open(&path).expect("旧库应能打开（可重复打开）");
        let msgs = store.list_messages("c1", None).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "旧消息");
        drop(store);
        assert_eq!(has_card_column(), 1, "打开后应补上 card_json 列");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
