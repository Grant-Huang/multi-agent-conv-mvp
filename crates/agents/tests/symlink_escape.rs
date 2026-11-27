//! RT-SYMLN（#500 / R-20261127-H4）：文件工具不得跟随工作区内的符号链接读写区外。
//! 工作区里的 `link -> 外部目录` 意图借道读/写/列出区外文件时，一律报「路径越界」，
//! 外部目录不得有任何新增文件；写文件拒绝以符号链接本身为目标。

use an_agents::ToolRegistry;
use serde_json::json;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

fn setup() -> (&'static str, PathBuf, ToolRegistry) {
    static GATE: OnceLock<()> = OnceLock::new();
    let _gate = GATE.get_or_init(|| ());
    let tag = an_core::new_id("symln");
    let dir = std::env::temp_dir().join(&tag);
    std::fs::create_dir_all(dir.join("workspace")).unwrap();
    let outside = dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "TOP-SECRET").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, dir.join("workspace").join("link")).unwrap();
    let tools = ToolRegistry::with_builtin(&dir.join("workspace")).unwrap();
    let leaked: &'static str = Box::leak(tag.into_boxed_str());
    (leaked, outside, tools)
}

fn cleanup(tag: &str) {
    std::thread::sleep(Duration::from_millis(10));
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join(tag));
}

/// read_file 借符号链接读区外 → 拒绝
#[tokio::test]
async fn rt_symln_01_read_via_symlink_rejected() {
    let (tag, _outside, tools) = setup();
    let err = tools
        .call("read_file", &json!({"path": "link/secret.txt"}))
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("路径越界"),
        "应拒绝符号链接借道读取：{err:#}"
    );
    cleanup(tag);
}

/// write_file 借符号链接写区外 → 拒绝，外部目录无新增文件
#[tokio::test]
async fn rt_symln_02_write_via_symlink_rejected() {
    let (tag, outside, tools) = setup();
    let err = tools
        .call(
            "write_file",
            &json!({"path": "link/pwned.txt", "content": "pwned"}),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("路径越界"),
        "应拒绝符号链接借道写入：{err:#}"
    );
    assert!(
        !outside.join("pwned.txt").exists(),
        "外部目录不得出现新文件"
    );
    cleanup(tag);
}

/// list_files 借符号链接列区外 → 拒绝
#[tokio::test]
async fn rt_symln_03_list_via_symlink_rejected() {
    let (tag, _outside, tools) = setup();
    let err = tools
        .call("list_files", &json!({"path": "link"}))
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("路径越界"),
        "应拒绝符号链接借道列出：{err:#}"
    );
    cleanup(tag);
}

/// write_file 以符号链接本身为目标（即使指向界内）→ 拒绝
#[tokio::test]
async fn rt_symln_04_write_to_symlink_target_rejected() {
    let (tag, _outside, tools) = setup();
    let ws = std::env::temp_dir().join(tag).join("workspace");
    std::fs::write(ws.join("a.txt"), "original").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(ws.join("a.txt"), ws.join("link2")).unwrap();
    let err = tools
        .call(
            "write_file",
            &json!({"path": "link2", "content": "replaced"}),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("拒绝写入符号链接目标"),
        "应拒绝以链接为写入目标：{err:#}"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).unwrap(),
        "original",
        "链接目标文件不得被改写"
    );
    cleanup(tag);
}

/// 反向断言：合法路径不受影响（读写列出照常）
#[tokio::test]
async fn rt_symln_05_normal_paths_still_work() {
    let (tag, _outside, tools) = setup();
    tools
        .call(
            "write_file",
            &json!({"path": "ok.md", "content": "正常内容"}),
        )
        .await
        .unwrap();
    let out = tools
        .call("read_file", &json!({"path": "ok.md"}))
        .await
        .unwrap();
    assert_eq!(out.text, "正常内容");
    let listed = tools
        .call("list_files", &json!({"path": ""}))
        .await
        .unwrap();
    assert!(
        listed.text.contains("ok.md"),
        "列表应含合法文件：{}",
        listed.text
    );
    cleanup(tag);
}
