use an_agents::util::{collect_git_changes, snapshot_worktree};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let p = std::env::temp_dir().join(an_core::new_id("git evidence spaces"));
        std::fs::create_dir_all(&p).unwrap();
        let r = Self(p);
        git(&r.0, &["init", "-q"]);
        r
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn git(p: &Path, args: &[&str]) -> Vec<u8> {
    let o = Command::new("git")
        .arg("-C")
        .arg(p)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&o.stderr)
    );
    o.stdout
}
fn seed(r: &Repo) {
    std::fs::write(r.0.join("tracked.txt"), "base\n").unwrap();
    std::fs::write(r.0.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&r.0, &["add", "."]);
    git(
        &r.0,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "base",
        ],
    );
}
fn index_path(p: &Path) -> PathBuf {
    p.join(
        String::from_utf8(git(p, &["rev-parse", "--git-path", "index"]))
            .unwrap()
            .trim(),
    )
}

#[tokio::test]
async fn rt_git_01_fallback_preserves_real_index_and_collects_untracked() {
    let r = Repo::new();
    seed(&r);
    std::fs::write(r.0.join("tracked.txt"), "staged\n").unwrap();
    git(&r.0, &["add", "tracked.txt"]);
    std::fs::write(r.0.join("tracked.txt"), "unstaged\n").unwrap();
    std::fs::write(r.0.join("intent.txt"), "intent\n").unwrap();
    git(&r.0, &["add", "-N", "intent.txt"]);
    std::fs::write(r.0.join("new.txt"), "new\n").unwrap();
    std::fs::write(r.0.join("ignored.txt"), "secret\n").unwrap();
    let before = std::fs::read(index_path(&r.0)).unwrap();
    let cached = git(&r.0, &["diff", "--cached"]);
    let flags = git(&r.0, &["ls-files", "--stage", "--debug"]);
    let (e, a) = collect_git_changes(&r.0, None).await;
    assert_eq!(
        std::fs::read(index_path(&r.0)).unwrap(),
        before,
        "采集不得修改用户真实索引"
    );
    assert_eq!(git(&r.0, &["diff", "--cached"]), cached);
    assert_eq!(git(&r.0, &["ls-files", "--stage", "--debug"]), flags);
    assert!(e.iter().any(|x| x.content.contains("new.txt")));
    let diff = &a.iter().find(|x| x.kind == "diff").unwrap().content;
    assert!(diff.contains("new.txt") && diff.contains("unstaged"));
    assert!(!diff.contains("ignored.txt"));
    assert_eq!(
        std::fs::read_to_string(r.0.join("tracked.txt")).unwrap(),
        "unstaged\n"
    );
    assert!(a
        .iter()
        .any(|x| x.kind == "file" && x.name == "new.txt" && x.content == "new\n"));
}

#[tokio::test]
async fn rt_git_02_fallback_unborn_repo_keeps_index_absent() {
    let r = Repo::new();
    std::fs::write(r.0.join("new.txt"), "new\n").unwrap();
    let (e, a) = collect_git_changes(&r.0, None).await;
    assert!(!index_path(&r.0).exists(), "空仓库采集不得创建真实索引");
    assert!(e.iter().any(|x| x.content.contains("new.txt")));
    assert!(a.iter().any(|x| x.kind == "file" && x.name == "new.txt"));
}

#[tokio::test]
async fn rt_git_03_linked_worktree_concurrent_collection_preserves_index() {
    let r = Repo::new();
    seed(&r);
    let linked = r.0.join("linked worktree");
    git(
        &r.0,
        &["worktree", "add", "--detach", linked.to_str().unwrap()],
    );
    std::fs::write(linked.join("new.txt"), "new\n").unwrap();
    let before = std::fs::read(index_path(&linked)).unwrap();
    let (a, b) = tokio::join!(
        collect_git_changes(&linked, None),
        collect_git_changes(&linked, None)
    );
    assert_eq!(
        std::fs::read(index_path(&linked)).unwrap(),
        before,
        "链接 worktree 的真实索引必须保持不变"
    );
    for (_, artifacts) in [a, b] {
        assert!(artifacts.iter().any(|x| x.name == "new.txt"));
    }
    assert!(snapshot_worktree(&linked).await.is_some());
    assert_eq!(std::fs::read(index_path(&linked)).unwrap(), before);
}

#[tokio::test]
async fn rt_git_04_failed_collection_reports_unavailable_without_mutating_index() {
    let r = Repo::new();
    seed(&r);
    std::fs::write(index_path(&r.0), b"invalid index").unwrap();
    let (e, a) = collect_git_changes(&r.0, None).await;
    assert_eq!(std::fs::read(index_path(&r.0)).unwrap(), b"invalid index");
    assert!(a.is_empty());
    assert!(
        e.iter().any(|x| x.content.contains("不可用")),
        "Git 失败必须明确说明证据不可用：{e:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn rt_git_05_cancelled_snapshot_reaps_filter_and_removes_private_index() {
    let r = Repo::new();
    seed(&r);
    let marker = r.0.join("filter-marker");
    let script = r.0.join("slow-filter.sh");
    std::fs::write(
        &script,
        format!(
            "printf '%s\\n%s\\n' \"$$\" \"$GIT_INDEX_FILE\" > '{}'\nsleep 120\ncat\n",
            marker.display()
        ),
    )
    .unwrap();
    git(
        &r.0,
        &[
            "config",
            "filter.slow.clean",
            &format!("sh '{}'", script.display()),
        ],
    );
    git(&r.0, &["config", "filter.slow.required", "true"]);
    std::fs::write(r.0.join(".gitattributes"), "tracked.txt filter=slow\n").unwrap();
    std::fs::write(r.0.join("tracked.txt"), "changed\n").unwrap();
    let before = std::fs::read(index_path(&r.0)).unwrap();
    let dir = r.0.clone();
    let task = tokio::spawn(async move { snapshot_worktree(&dir).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let content = std::fs::read_to_string(&marker).unwrap();
    let mut lines = content.lines();
    let pid: i32 = lines.next().unwrap().parse().unwrap();
    let private_index = PathBuf::from(lines.next().unwrap());
    task.abort();
    let _ = task.await;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            // 与仓库 test_proc::alive 一致：容器 init 可能尚未回收已退出的孙进程。
            let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|s| {
                    s.rsplit_once(')')
                        .map(|(_, r)| r.trim_start().starts_with('Z'))
                })
                == Some(false);
            if !alive && !private_index.parent().unwrap().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("取消后必须清理过滤器进程、私有索引与锁目录");
    assert_eq!(std::fs::read(index_path(&r.0)).unwrap(), before);
}
