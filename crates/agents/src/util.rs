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
    git_with_index(workdir, None, args).await
}

/// 同上；`index` 指定时使用这个临时暂存区文件，不影响仓库自己的暂存区
async fn git_with_index(workdir: &Path, index: Option<&Path>, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(workdir).args(args);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let out = cmd.output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// 给工作区（含未跟踪文件，遵守 .gitignore）拍快照，返回 git 树对象的哈希。
/// 任务开始前调用，结束后用 [`collect_git_changes`] 只对比任务期间的改动。
/// 不是 git 仓库或 git 出错时返回 None。
pub async fn snapshot_worktree(workdir: &Path) -> Option<String> {
    git(workdir, &["rev-parse", "--is-inside-work-tree"]).await?;
    let tmp_index = std::env::temp_dir().join(an_core::new_id("an-git-index"));
    // 从仓库暂存区复制一份作为起点，git 可以复用其中的文件状态缓存，大仓库也很快
    if let Some(p) = git(workdir, &["rev-parse", "--git-path", "index"]).await {
        let real_index = workdir.join(p.trim());
        let _ = tokio::fs::copy(&real_index, &tmp_index).await;
    }
    let tree = match git_with_index(workdir, Some(&tmp_index), &["add", "-A"]).await {
        Some(_) => git_with_index(workdir, Some(&tmp_index), &["write-tree"]).await.map(|s| s.trim().to_string()),
        None => None,
    };
    let _ = tokio::fs::remove_file(&tmp_index).await;
    tree.filter(|t| !t.is_empty())
}

/// 收集工作目录的代码改动：`git diff --stat` 作为证据，完整 diff 作为产物。
/// "Agent 说改好了"不等于"改对了"，审查要看证据。
/// `baseline` 为任务开始前 [`snapshot_worktree`] 的结果：有它时只包含任务期间的改动，
/// 没有时退回到与 HEAD 比较（会包含任务开始前就存在的未提交改动）。
pub async fn collect_git_changes(workdir: &Path, baseline: Option<&str>) -> (Vec<Evidence>, Vec<Artifact>) {
    let mut evidence = vec![];
    let mut artifacts = vec![];
    if git(workdir, &["rev-parse", "--is-inside-work-tree"]).await.is_none() {
        evidence.push(Evidence { kind: "git_diff_stat".into(), content: "工作目录不是 git 仓库，无法收集代码改动".into() });
        return (evidence, artifacts);
    }
    let after = match baseline {
        Some(_) => snapshot_worktree(workdir).await,
        None => None,
    };
    let (stat, diff) = match (baseline, after.as_deref()) {
        (Some(before), Some(after)) => (git(workdir, &["diff", "--stat", before, after]).await, git(workdir, &["diff", before, after]).await),
        _ => {
            // intent-to-add：让新建的未跟踪文件也出现在 diff 里（不会真正暂存内容）
            let _ = git(workdir, &["add", "-N", "."]).await;
            (git(workdir, &["diff", "--stat"]).await, git(workdir, &["diff"]).await)
        }
    };
    if let Some(stat) = stat {
        evidence.push(Evidence { kind: "git_diff_stat".into(), content: if stat.trim().is_empty() { "无改动".into() } else { stat } });
    }
    if let Some(diff) = diff.filter(|d| !d.trim().is_empty()) {
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

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(an_core::new_id("an-util-git"));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(&dir).args(args).output().unwrap();
        git(&["init", "-q"]);
        std::fs::write(dir.join("tracked.txt"), "v1\n").unwrap();
        git(&["add", "tracked.txt"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn diff_only_contains_changes_after_snapshot() {
        let dir = temp_repo();
        // 任务开始前就存在、尚未提交的改动：不属于本次任务
        std::fs::write(dir.join("old_untracked.py"), "print('old')\n").unwrap();
        let index_before = std::fs::read(dir.join(".git/index")).unwrap();

        let baseline = snapshot_worktree(&dir).await;
        assert!(baseline.is_some(), "git 仓库应能生成快照");

        // 任务期间的改动：新建文件 + 修改已提交文件
        std::fs::write(dir.join("new_file.py"), "print('new')\n").unwrap();
        std::fs::write(dir.join("tracked.txt"), "v2\n").unwrap();

        let (evidence, artifacts) = collect_git_changes(&dir, baseline.as_deref()).await;
        let diff = &artifacts[0].content;
        assert!(diff.contains("new_file.py") && diff.contains("tracked.txt"), "{diff}");
        assert!(!diff.contains("old_untracked.py"), "任务开始前已有的改动不应计入：{diff}");
        let stat = &evidence.iter().find(|e| e.kind == "git_diff_stat").unwrap().content;
        assert!(stat.contains("new_file.py") && !stat.contains("old_untracked.py"), "{stat}");
        assert_eq!(std::fs::read(dir.join(".git/index")).unwrap(), index_before, "不应改动用户仓库的暂存区");
    }

    #[tokio::test]
    async fn no_changes_after_snapshot() {
        let dir = temp_repo();
        std::fs::write(dir.join("old_untracked.py"), "print('old')\n").unwrap();
        let baseline = snapshot_worktree(&dir).await;
        let (evidence, artifacts) = collect_git_changes(&dir, baseline.as_deref()).await;
        assert!(artifacts.is_empty(), "没有新改动时不应有 diff 产物");
        assert_eq!(evidence.iter().find(|e| e.kind == "git_diff_stat").unwrap().content, "无改动");
    }

    #[tokio::test]
    async fn snapshot_outside_git_repo() {
        let dir = std::env::temp_dir().join(an_core::new_id("an-util-nogit"));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(snapshot_worktree(&dir).await.is_none());
        let (evidence, artifacts) = collect_git_changes(&dir, None).await;
        assert!(artifacts.is_empty());
        assert!(evidence[0].content.contains("不是 git 仓库"));
    }
}
