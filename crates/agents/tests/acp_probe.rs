//! RT-AGCAT-10：握手检测在"进程一启动就退出"时的分类回归用例。
//!
//! 背景（2026-11-18 CI 偶发失败）：缺 node 时 `env` 打印错误后立刻退出，我们写握手请求时会遇到
//! Broken pipe。这个写失败的错误有时先于"进程退出 + 错误输出末尾"返回，分类时看不到错误输出，
//! 就把"缺依赖"误判成"握手失败"。本地复现约两成概率，所以这里连续 50 轮、每轮 8 个并发检测。
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use an_agents::acp::{AcpConfig, PermissionMode};
use an_agents::AcpAgent;
use an_core::{ProbeKind, Role};

/// 临时目录：放假的 agent 脚本，用完删除
struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(an_core::new_id(&format!("acp-probe-{tag}")));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    /// 写一个可执行的 sh 脚本，返回它的完整路径
    fn script(&self, name: &str, body: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 用脚本路径构造一个只用于检测的 ACP agent
fn agent(dir: &Dir, command: String) -> AcpAgent {
    AcpAgent::new(
        AcpConfig {
            id: "probe-test".into(),
            name: None,
            command,
            args: vec![],
            env: Default::default(),
            role: Role::Coding,
            agent_type: None,
            max_concurrent: 1,
            workdir: None,
            timeout_secs: 60,
            permission: PermissionMode::DenyAll,
            install_hint: None,
        },
        dir.0.join("work"),
    )
}

// 多线程运行时：与服务端一致（单线程运行时下这个竞争几乎不出现）
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_agcat_10_exit_before_handshake_is_classified_by_stderr() {
    let dir = Dir::new("10");

    // 1) 缺 node：打印错误后立刻退出。400 次检测都必须判为"缺依赖"
    let a = agent(
        &dir,
        dir.script(
            "needs-node",
            "echo 'env: node: No such file or directory' >&2\nexit 127",
        ),
    );
    // 每轮 8 个检测同时进行（服务端启动时后台检测与用户触发的检测会并发，调度更紧张，竞争更容易出现）
    let a = std::sync::Arc::new(a);
    let mut wrong = vec![];
    for round in 0..50 {
        let outs = futures::future::join_all((0..8).map(|_| {
            let a = a.clone();
            async move { a.probe_detailed(Duration::from_secs(10)).await }
        }))
        .await;
        for (i, out) in outs.into_iter().enumerate() {
            if out.kind != ProbeKind::MissingDependency {
                wrong.push(format!(
                    "第 {round} 轮第 {i} 个：{:?} {:?}",
                    out.kind, out.detail
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{} 次分类错误：{wrong:#?}", wrong.len());

    // 2) 不写任何错误输出就退出：仍是"握手失败"，且补等错误输出有上限，不拖慢检测
    let b = agent(&dir, dir.script("silent-exit", "exit 1"));
    let t = Instant::now();
    let out = b.probe_detailed(Duration::from_secs(10)).await;
    assert_eq!(out.kind, ProbeKind::HandshakeFailed, "{:?}", out.detail);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "检测耗时 {:?}，补等错误输出应有上限",
        t.elapsed()
    );
}

fn mock_acp_binary() -> PathBuf {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace.join("target"));
    let target = if target.is_absolute() {
        target
    } else {
        workspace.join(target)
    };
    target.join("debug/an-mock-acp")
}

/// probe 握手时传给 `session/new` 的工作目录必须为绝对路径（与 `run()` 一致）
#[tokio::test]
async fn probe_session_new_cwd_is_canonical() {
    let mock = mock_acp_binary();
    assert!(
        mock.is_file(),
        "缺少 {}，请先 cargo build -p an-server --bin an-mock-acp",
        mock.display()
    );
    let dir = Dir::new("cwd");
    let base = dir.0.clone();
    let rel = PathBuf::from("workspace/code");
    std::fs::create_dir_all(base.join(&rel)).unwrap();
    let probe_cwd_file = base.join("probe-cwd.txt");
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base).unwrap();

    let mut env = BTreeMap::new();
    env.insert(
        "AN_MOCK_ACP_PROBE_CWD".into(),
        probe_cwd_file.to_string_lossy().into_owned(),
    );
    let a = AcpAgent::new(
        AcpConfig {
            id: "probe-cwd".into(),
            name: None,
            command: mock.to_string_lossy().into_owned(),
            args: vec![],
            env,
            role: Role::Coding,
            agent_type: None,
            max_concurrent: 1,
            workdir: Some(rel.clone()),
            timeout_secs: 60,
            permission: PermissionMode::DenyAll,
            install_hint: None,
        },
        base.join("unused-default"),
    );

    let out = a.probe_detailed(Duration::from_secs(10)).await;
    std::env::set_current_dir(prev_cwd).unwrap();

    assert_eq!(out.kind, ProbeKind::Ok, "{:?}", out.detail);
    let recorded = std::fs::read_to_string(&probe_cwd_file).unwrap();
    let recorded_path = PathBuf::from(recorded.trim());
    assert!(
        recorded_path.is_absolute(),
        "probe 应发送绝对路径，实际为 {:?}",
        recorded_path
    );
    assert_eq!(
        recorded_path,
        base.join(&rel).canonicalize().unwrap(),
        "canonical 路径应与相对 workdir 解析结果一致"
    );
}
