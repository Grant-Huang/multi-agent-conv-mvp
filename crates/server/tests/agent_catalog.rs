//! HTTP 用例 RT-AGCAT-01 ~ RT-AGCAT-09（定义见 docs/testing/CASES.md「Agent 目录与握手检测」）：
//! `GET /api/agents/catalog` 列出预设目录与状态，`POST /api/agents/probe` 握手并给出分类结果。
//!
//! 全部通过 HTTP 驱动，用内存库和不可连接的模型地址（这些接口不调用模型）。
//! 需要"已安装 / 未安装"的用例会把 PATH、HOME 指向临时目录；
//! 它们是进程级全局变量，所以本文件所有用例用同一把锁串行执行。本文件是独立测试二进制。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

/// 用 tokio 的锁：用例在持锁期间会 await HTTP 请求
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 自带的模拟 ACP agent（握手正常）
const MOCK: &str = env!("CARGO_BIN_EXE_an-mock-acp");

/// 本用例独享的临时目录：bin/ 放假命令或脚本，home/ 当 HOME
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "an-agcat-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        Self { root }
    }

    /// 在 bin/ 下写一个可执行脚本
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let p = self.root.join("bin").join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// 在 PATH = sandbox/bin、HOME = sandbox/home 的环境下构造 Runtime（只在构造时读取 PATH），
/// 构造完立即恢复环境变量，然后启动 HTTP 服务。
async fn serve(sb: &Sandbox, extra_config: &str) -> (String, Arc<Runtime>) {
    let text = format!(
        r#"
[llm]
base_url = "http://127.0.0.1:9/v1"
model = "m"
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
workdir = "{code}"
{extra_config}
"#,
        files = sb.root.join("files").display(),
        code = sb.root.join("code").display(),
    );
    let (config, _) = AppConfig::parse(&text).unwrap();
    let old_path = std::env::var_os("PATH");
    let old_home = std::env::var_os("HOME");
    std::env::set_var("PATH", sb.root.join("bin"));
    std::env::set_var("HOME", sb.root.join("home"));
    let rt = Runtime::new(config, Store::open(Path::new(":memory:")).unwrap());
    restore("PATH", old_path);
    restore("HOME", old_home);
    let rt = Arc::new(rt.unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let for_serve = rt.clone();
    tokio::spawn(async move {
        axum::serve(listener, an_server::api(for_serve))
            .await
            .unwrap()
    });
    (format!("http://{addr}"), rt)
}

fn restore(key: &str, old: Option<std::ffi::OsString>) {
    match old {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

async fn get_json(url: String) -> (u16, Value) {
    let r = reqwest::get(url).await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn probe(base: &str, body: Value) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/agents/probe"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

fn item<'a>(catalog: &'a Value, id: &str) -> &'a Value {
    catalog
        .as_array()
        .unwrap_or_else(|| panic!("目录接口应返回数组，实际：{catalog}"))
        .iter()
        .find(|i| i["id"] == id)
        .unwrap_or_else(|| panic!("目录里缺少 {id}：{catalog}"))
}

/// 注册一个指向 `command` 的 ACP agent（自动发现关闭）
fn acp_agent(id: &str, command: &Path, env: &str) -> String {
    format!(
        "[acp]\nauto_detect = false\n[[acp.agents]]\nid = \"{id}\"\ncommand = \"{}\"\nrole = \"coding\"\n{env}\n",
        command.display()
    )
}

/// RT-AGCAT-01：目录包含全部预设；已安装且自动发现 → registered；未安装 → not_installed
/// （#374：目录探测按 Issue #325 口径实时读 PATH——目录请求保持在沙箱 PATH 下
/// 发出，消除对宿主机是否安装 codex-acp 的依赖）
#[tokio::test]
async fn rt_agcat_01_catalog_lists_presets_with_status() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("01");
    sb.script("claude-agent-acp", "exit 0");
    let (base, _rt) = serve(&sb, "[acp]\nauto_detect = true").await;
    // 目录探测在请求时实时解析 PATH（构造期沙箱已恢复）——把 PATH/HOME 重新
    // 指回沙箱再发请求，宿主机安装状态不再影响断言
    let old_path = std::env::var_os("PATH");
    let old_home = std::env::var_os("HOME");
    std::env::set_var("PATH", sb.root.join("bin"));
    std::env::set_var("HOME", sb.root.join("home"));
    let (status, catalog) = get_json(format!("{base}/api/agents/catalog")).await;
    restore("PATH", old_path);
    restore("HOME", old_home);
    assert_eq!(status, 200, "目录接口应可用：{catalog}");
    let ids: Vec<&str> = catalog
        .as_array()
        .unwrap_or_else(|| panic!("目录接口应返回数组，实际：{catalog}"))
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "opencode",
            "hermes",
            "pi",
            "claude",
            "codex-acp",
            "cursor",
            "zcode"
        ],
        "目录应按预设顺序列出全部 7 个预设"
    );
    let claude = item(&catalog, "claude");
    assert_eq!(claude["status"], "registered", "{claude}");
    assert!(
        claude["link_status"].is_string(),
        "已接入的 agent 应带连接状态：{claude}"
    );
    assert_eq!(item(&catalog, "codex-acp")["status"], "not_installed");
}

/// RT-AGCAT-02：每一项都有名称、用途、角色、启动命令和安装步骤
#[tokio::test]
async fn rt_agcat_02_every_item_is_self_explaining() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("02");
    let (base, _rt) = serve(&sb, "").await;
    let (status, catalog) = get_json(format!("{base}/api/agents/catalog")).await;
    assert_eq!(status, 200, "目录接口应可用：{catalog}");
    let items = catalog
        .as_array()
        .unwrap_or_else(|| panic!("目录接口应返回数组，实际：{catalog}"));
    assert!(!items.is_empty(), "目录不应为空");
    for i in items {
        for key in ["name", "purpose", "role", "command"] {
            assert!(
                i[key].as_str().is_some_and(|s| !s.is_empty()),
                "{} 缺少 {key}：{i}",
                i["id"]
            );
        }
        let steps = i["install"].as_array();
        assert!(
            steps.is_some_and(|s| !s.is_empty()
                && s.iter()
                    .all(|st| st["text"].as_str().is_some_and(|t| !t.is_empty()))),
            "{} 应至少有一条安装步骤：{i}",
            i["id"]
        );
    }
}

/// RT-AGCAT-03：握手正常 → ok
#[tokio::test]
async fn rt_agcat_03_probe_ok() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("03");
    let (base, _rt) = serve(&sb, &acp_agent("mock", Path::new(MOCK), "")).await;
    let (status, out) = probe(&base, json!({"id": "mock"})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["kind"], "ok", "{out}");
}

/// RT-AGCAT-04：agent 返回 -32000 → needs_login，并告诉用户下一步
#[tokio::test]
async fn rt_agcat_04_probe_needs_login() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("04");
    let cfg = acp_agent(
        "mock-auth",
        Path::new(MOCK),
        "env = { AN_MOCK_ACP_AUTH = \"required\" }",
    );
    let (base, _rt) = serve(&sb, &cfg).await;
    let (status, out) = probe(&base, json!({"id": "mock-auth"})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["kind"], "needs_login", "{out}");
    assert!(
        out["next_step"].as_str().is_some_and(|s| !s.is_empty()),
        "需要登录时应给出下一步：{out}"
    );
}

/// RT-AGCAT-05：进程因找不到 node 退出 → missing_dependency，详情保留错误输出
#[tokio::test]
async fn rt_agcat_05_probe_missing_dependency() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("05");
    let s = sb.script(
        "needs-node",
        "echo 'env: node: No such file or directory' >&2\nexit 127",
    );
    let (base, _rt) = serve(&sb, &acp_agent("needs-node", &s, "")).await;
    let (status, out) = probe(&base, json!({"id": "needs-node"})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["kind"], "missing_dependency", "{out}");
    assert!(
        out["detail"].as_str().is_some_and(|d| d.contains("node")),
        "详情应保留错误输出：{out}"
    );
}

/// RT-AGCAT-06：不响应 → 约 10 秒后 timeout，进程被结束
#[tokio::test]
async fn rt_agcat_06_probe_timeout_kills_process() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("06");
    let pid_file = sb.root.join("pid");
    let s = sb.script(
        "silent",
        &format!("echo $$ > '{}'\nexec sleep 60", pid_file.display()),
    );
    let (base, _rt) = serve(&sb, &acp_agent("silent", &s, "")).await;
    let started = Instant::now();
    let (status, out) = probe(&base, json!({"id": "silent"})).await;
    let took = started.elapsed();
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["kind"], "timeout", "{out}");
    assert!(
        took < Duration::from_secs(20),
        "应在超时后及时返回，实际 {took:?}"
    );
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("脚本应已启动并写下 pid")
        .trim()
        .parse()
        .unwrap();
    assert!(wait_dead(pid, 5), "超时后进程 {pid} 应已结束");
}

/// RT-AGCAT-07：目录内预设未安装 → not_installed，立即返回
#[tokio::test]
async fn rt_agcat_07_probe_not_installed() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("07");
    let (base, _rt) = serve(&sb, "[acp]\nauto_detect = true").await;
    let started = Instant::now();
    let (status, out) = probe(&base, json!({"id": "zcode"})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["kind"], "not_installed", "{out}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "未安装时不应等待"
    );
}

/// RT-AGCAT-08：未知 id / 非 ACP agent → 404；请求体带 command → 拒绝且不执行
#[tokio::test]
async fn rt_agcat_08_probe_rejects_unknown_and_commands() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("08");
    let marker = sb.root.join("executed");
    let evil = sb.script("evil", &format!("touch '{}'", marker.display()));
    let (base, _rt) = serve(&sb, "").await;

    let (status, out) = probe(&base, json!({"id": "no-such-agent"})).await;
    assert_eq!(status, 404, "未知 id 应返回 404：{out}");
    let (status, out) = probe(&base, json!({"id": "general"})).await;
    assert_eq!(status, 404, "非 ACP agent 应返回 404：{out}");

    let (status, out) = probe(
        &base,
        json!({"id": "no-such-agent", "command": evil.display().to_string()}),
    )
    .await;
    assert!(
        (400..500).contains(&status) && status != 404,
        "带 command 字段的请求应被拒绝（4xx，非 404），实际 {status}：{out}"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!marker.exists(), "请求里的命令不应被执行");
}

/// RT-AGCAT-09：检测已注册 agent 成功后，健康接口的连接状态变为 connected
#[tokio::test]
async fn rt_agcat_09_probe_updates_link_status() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("09");
    let (base, _rt) = serve(&sb, &acp_agent("mock", Path::new(MOCK), "")).await;
    let link = |health: &Value| {
        health["agents"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["id"] == "mock"))
            .map(|x| x["link"]["status"].clone())
            .unwrap_or(Value::Null)
    };
    let (_, before) = get_json(format!("{base}/api/health")).await;
    assert_eq!(link(&before), "configured", "检测前应为未连通：{before}");
    let (status, out) = probe(&base, json!({"id": "mock"})).await;
    assert_eq!(status, 200, "{out}");
    let (_, after) = get_json(format!("{base}/api/health")).await;
    assert_eq!(link(&after), "connected", "检测成功后应为已连通：{after}");
}

/// 进程是否已结束（不存在或僵尸都算结束）；最多等 `secs` 秒。
/// Linux 读 /proc；没有 /proc 的系统（macOS）用 `kill -0` 判断。
fn wait_dead(pid: u32, secs: u64) -> bool {
    let alive = || {
        if Path::new("/proc/self").exists() {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Ok(stat) => stat
                    .rsplit_once(')')
                    .and_then(|(_, rest)| rest.split_whitespace().next())
                    .is_some_and(|state| state != "Z"),
                Err(_) => false,
            }
        } else {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .is_ok_and(|s| s.success())
        }
    };
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if !alive() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !alive()
}
