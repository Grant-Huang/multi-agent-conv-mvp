//! RT-AGCAT-10：握手检测在"进程一启动就退出"时的分类回归用例。
//!
//! 背景（2026-11-18 CI 偶发失败）：缺 node 时 `env` 打印错误后立刻退出，我们写握手请求时会遇到
//! Broken pipe。这个写失败的错误有时先于"进程退出 + 错误输出末尾"返回，分类时看不到错误输出，
//! 就把"缺依赖"误判成"握手失败"。本地复现约两成概率，所以这里连续 50 轮、每轮 8 个并发检测。
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use an_agents::acp::{AcpConfig, PermissionMode};
use an_agents::AcpAgent;
use an_core::{ProbeKind, ProbeOutcome, Role};

/// exec 偶发瞬态 ETXTBSY 的有界重试（Q-AR2-04 第四样本，Issue #343）。
///
/// CI（rust job 单进程内三测试并行）出现过一次：spawn 已拷贝好的 mock 二进制时报
/// `无法启动：Text file busy (os error 26)`——同提交 rerun 转绿、本地不触发，属瞬态写持有窗口
/// （构建/拷贝刚结束时的内核写计数落账延迟一类），几百毫秒内自愈。
/// 重试只吸收这个窗口，总数有上限；断言口径不变：最终仍要求探测成功（Ok）。
const BUSY_RETRIES: usize = 40;
const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// 探测一次；仅当失败原因是「Text file busy」时按上限重试，其他失败原样返回。
async fn probe_with_busy_retry(a: &AcpAgent, timeout: Duration) -> ProbeOutcome {
    let mut out = a.probe_detailed(timeout).await;
    for _ in 0..BUSY_RETRIES {
        let busy = out
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("Text file busy"));
        if !busy {
            return out;
        }
        tokio::time::sleep(BUSY_RETRY_INTERVAL).await;
        out = a.probe_detailed(timeout).await;
    }
    out
}

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

    let out = probe_with_busy_retry(&a, Duration::from_secs(10)).await;
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

/// Issue #325：适配器创建时命令不存在 → 之后命令出现在磁盘（模拟 npm install -g）
/// → 再次探测应重新解析并完成握手，而不是永远 not_installed
/// （此前体检能找到命令、检测却报 not_installed 的矛盾根因）。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rt_i325_01_probe_reresolves_command_appearing_after_construction() {
    let dir = Dir::new("i325");
    let late = dir.0.join("late-acp");
    let mock = mock_acp_binary();
    assert!(
        mock.is_file(),
        "缺少 {}，请先 cargo build -p an-server --bin an-mock-acp",
        mock.display()
    );

    // 1) 创建适配器时命令还不存在
    let a = Arc::new(agent(&dir, late.to_string_lossy().into_owned()));
    let out = a.probe_detailed(Duration::from_secs(10)).await;
    assert_eq!(
        out.kind,
        ProbeKind::NotInstalled,
        "命令不存在时应判 not_installed：{:?}",
        out.detail
    );
    assert!(!a.is_installed(), "创建时未安装");

    // 2) 之后命令出现在磁盘（npm install -g 的时序模拟），内容为可完成 ACP 握手的真二进制
    std::fs::copy(&mock, &late).unwrap();
    std::fs::set_permissions(&late, std::fs::Permissions::from_mode(0o755)).unwrap();

    // 3) 再次探测：重新解析 + 握手成功（不再永远 not_installed）。
    //    exec 换成有界重试：吸收 CI 偶发的瞬态 ETXTBSY 写持有窗口（#343），断言仍是握手必须 Ok
    let out = probe_with_busy_retry(&a, Duration::from_secs(10)).await;
    assert_eq!(
        out.kind,
        ProbeKind::Ok,
        "命令出现后再次探测应重新解析并握手成功：{:?} {:?}",
        out.kind,
        out.detail
    );
    assert!(a.is_installed(), "探测自愈后 is_installed 应与体检口径一致");
}
