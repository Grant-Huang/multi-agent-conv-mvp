//! 测试共用：找到 `an-des-mcp` 可执行文件（RT-DESC 系列）。

use serde_json::Value;
use std::path::PathBuf;

/// `an-des-mcp` 可执行文件：优先用环境变量 `AN_DES_MCP_BIN`；否则请 cargo 构建（已是最新时很快）并从构建输出中
/// 取可执行文件的实际路径。不按测试程序的位置推算路径：不同 cargo 版本与 CI 的构建目录布局不同。
pub fn des_mcp_bin() -> PathBuf {
    static BIN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BIN.get_or_init(|| {
        if let Ok(p) = std::env::var("AN_DES_MCP_BIN") {
            return PathBuf::from(p);
        }
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        // 在工作区根目录运行：CI 与本地都用相对的 CARGO_TARGET_DIR=target，测试的工作目录是 crates/server，
        // 不切换目录会在 crates/server/target 下重新构建一遍。
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = std::process::Command::new(cargo)
            .current_dir(workspace)
            .args([
                "build",
                "--locked",
                "-p",
                "an-des-mcp",
                "--bin",
                "an-des-mcp",
                "--message-format=json",
            ])
            .stderr(std::process::Stdio::inherit())
            .output()
            .expect("run cargo build for an-des-mcp");
        assert!(out.status.success(), "an-des-mcp must build");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|m| m["reason"] == "compiler-artifact")
            .find_map(|m| m["executable"].as_str().map(PathBuf::from))
            .expect("cargo reports the an-des-mcp executable")
    })
    .clone()
}
