//! Agent 之间共用的小工具：命令查找、`~` 展开、git 改动证据收集。

use std::path::{Path, PathBuf};

use an_core::{truncate_chars, Artifact, Evidence};
use tokio::process::Command;

/// 展开路径开头的 `~`（子进程不经过 shell，`~` 不会被自动展开）
pub fn expand_tilde(s: &str) -> String {
    match (s.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ if s == "~" => std::env::var("HOME").unwrap_or_else(|_| s.into()),
        _ => s.into(),
    }
}

/// 除 PATH 外额外查找的目录。
/// 从桌面应用或服务方式启动时 PATH 往往不完整，这些是 npm / pip / brew / cargo 等的常见安装位置。
const EXTRA_DIRS: &[&str] = &[
    "~/.npm-global/bin",
    "~/.local/bin",
    "~/.opencode/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "~/.hermes/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];

/// 查找可执行文件：
/// - 含路径分隔符（如 `~/bin/x`、`./x`、`/usr/bin/x`）时直接检查该路径；
/// - 否则依次在 PATH 和常见安装目录中查找。
///
/// 只检查文件是否存在且可执行，不会运行它（有些 agent 的 `--version` 会启动交互）。
pub fn resolve_command(cmd: &str) -> Option<PathBuf> {
    let expanded = expand_tilde(cmd);
    if expanded.contains('/') {
        let p = PathBuf::from(&expanded);
        return is_executable(&p).then_some(p);
    }
    let path_dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    let extra = EXTRA_DIRS.iter().map(|d| PathBuf::from(expand_tilde(d)));
    path_dirs.into_iter().chain(extra).map(|d| d.join(&expanded)).find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// 在工作目录执行 git 命令，返回 stdout（失败返回 None）
async fn git(workdir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(workdir).args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 收集工作目录的代码改动：`git diff --stat` 作为证据，完整 diff 作为产物。
/// "Agent 说改好了"不等于"改对了"，审查要看证据。
pub async fn collect_git_changes(workdir: &Path) -> (Vec<Evidence>, Vec<Artifact>) {
    let mut evidence = vec![];
    let mut artifacts = vec![];
    if git(workdir, &["rev-parse", "--is-inside-work-tree"]).await.is_none() {
        evidence.push(Evidence { kind: "git_diff_stat".into(), content: "工作目录不是 git 仓库，无法收集代码改动".into() });
        return (evidence, artifacts);
    }
    // intent-to-add：让新建的未跟踪文件也出现在 diff 里（不会真正暂存内容）
    let _ = git(workdir, &["add", "-N", "."]).await;
    if let Some(stat) = git(workdir, &["diff", "--stat"]).await {
        evidence.push(Evidence { kind: "git_diff_stat".into(), content: if stat.trim().is_empty() { "无改动".into() } else { stat } });
    }
    if let Some(diff) = git(workdir, &["diff"]).await.filter(|d| !d.trim().is_empty()) {
        artifacts.push(Artifact { kind: "diff".into(), name: "git diff".into(), content: truncate_chars(&diff, 20000) });
    }
    (evidence, artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_commands() {
        assert!(resolve_command("sh").is_some());
        assert!(resolve_command("/bin/sh").is_some());
        assert!(resolve_command("definitely-not-a-command-xyz").is_none());
        assert!(resolve_command("/no/such/file").is_none());
    }
}
