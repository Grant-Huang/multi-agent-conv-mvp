//! RT-SCHV（#431 / W1-D6）：库 schema 版本戳。
//! 新代码打开旧库（user_version=0）自动升戳；打开「更新版本写过的库」时
//! newer_schema_version() 返回库版本（main 启动横幅据此告知新数据不可读写）。

use an_runtime::store::{Store, SCHEMA_VERSION};

fn temp_db(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(an_core::new_id(&format!("schv-{tag}")));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("an.db")
}

fn user_version(path: &std::path::Path) -> u32 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// 新库：打开后版本戳 = 当前 SCHEMA_VERSION，无 newer 提示
#[test]
fn rt_schv_01_fresh_db_gets_stamped() {
    let path = temp_db("fresh");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.newer_schema_version(), None);
    drop(store);
    assert_eq!(user_version(&path), SCHEMA_VERSION);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// 旧库（有表但 user_version=0）：打开后升级到当前版本戳，功能不受影响
#[test]
fn rt_schv_02_legacy_db_is_upgraded_in_place() {
    let path = temp_db("legacy");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE conversations (id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL);
             INSERT INTO conversations VALUES ('c1', '旧会话', '2026-01-01T00:00:00Z');",
        )
        .unwrap();
    }
    assert_eq!(user_version(&path), 0, "前提：旧库版本戳为 0");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.newer_schema_version(), None, "旧库不算 newer");
    drop(store);
    assert_eq!(user_version(&path), SCHEMA_VERSION, "打开后应升戳");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// 更新版本写过的库（user_version > 本二进制）：open 成功不损坏数据，
/// newer_schema_version 返回库版本（启动横幅据此告知）
#[test]
fn rt_schv_03_newer_db_is_detected_and_reported() {
    let path = temp_db("newer");
    let store = Store::open(&path).unwrap();
    drop(store);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.newer_schema_version(),
        Some(SCHEMA_VERSION + 1),
        "应检出库版本更高"
    );
    drop(store);
    assert_eq!(
        user_version(&path),
        SCHEMA_VERSION + 1,
        "不得把更高版本戳改小"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
