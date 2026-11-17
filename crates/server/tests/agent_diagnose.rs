//! 用例 RT-AGDIAG-01 ~ 04、06（定义见 docs/testing/CASES.md「助手排查：规则库与本机体检」）：
//! `POST /api/agents/diagnose` 先握手检测，再做只读的本机体检，按规则给出大白话结论。不外发任何内容。
//!
//! PATH / HOME / SHELL 在整条用例期间指向临时目录（检测和体检都在处理请求时进行），
//! 用例之间用同一把锁串行；本文件是独立测试二进制。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;
use serde_json::{json, Value};

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Sandbox {
    root: PathBuf,
    old: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Sandbox {
    /// `login_path`：假的登录 shell 被调用时打印的 PATH
    fn new(name: &str, login_path: &str) -> Self {
        let root = std::env::temp_dir().join(an_core::new_id(&format!("agdiag-{name}")));
        for d in ["bin", "home", "login", "files", "code", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let login_path = login_path.replace("{login}", &root.join("login").display().to_string());
        let shell = root.join("fake-shell");
        write_exec(&shell, &format!("printf '%s' '{login_path}'"));
        let mut old = vec![];
        for (k, v) in [
            ("PATH", root.join("bin").into_os_string()),
            ("HOME", root.join("home").into_os_string()),
            ("SHELL", shell.into_os_string()),
        ] {
            old.push((k, std::env::var_os(k)));
            std::env::set_var(k, v);
        }
        Self { root, old }
    }

    fn bin(&self, name: &str, body: &str) -> PathBuf {
        let p = self.root.join("bin").join(name);
        write_exec(&p, body);
        p
    }

    async fn serve(&self, extra: &str) -> (String, Arc<Runtime>) {
        let text = format!(
            r#"
[server]
db_path = "{data}/agentnexus.db"
[llm]
base_url = "http://127.0.0.1:9/v1"
model = "m"
[agents.general]
workspace_dir = "{files}"
[agents.coding]
enabled = false
workdir = "{code}"
{extra}
"#,
            data = self.root.join("data").display(),
            files = self.root.join("files").display(),
            code = self.root.join("code").display(),
        );
        let (config, _) = AppConfig::parse(&text).unwrap();
        let rt =
            Arc::new(Runtime::new(config, Store::open(Path::new(":memory:")).unwrap()).unwrap());
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
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for (k, v) in self.old.drain(..) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        // 凭证文件可能被设为不可读，先恢复权限再删
        let _ = std::process::Command::new("chmod")
            .args(["-R", "u+rwx"])
            .arg(&self.root)
            .status();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write_exec(p: &Path, body: &str) {
    std::fs::write(p, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

async fn diagnose(base: &str, id: &str) -> (u16, Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/api/agents/diagnose"))
        .json(&json!({ "id": id }))
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

fn finding<'a>(report: &'a Value, rule: &str) -> Option<&'a Value> {
    report["findings"]
        .as_array()?
        .iter()
        .find(|f| f["rule"] == rule)
}

fn check<'a>(report: &'a Value, key: &str) -> Option<&'a Value> {
    report["checks"]
        .as_array()?
        .iter()
        .find(|c| c["key"] == key)
}

/// RT-AGDIAG-01：错误输出里有"找不到 node" → 缺依赖，命中规则并给出安装步骤
#[tokio::test]
async fn rt_agdiag_01_node_missing_rule() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("01", "/usr/bin");
    sb.bin(
        "claude-agent-acp",
        "echo 'env: node: No such file or directory' >&2\nexit 127",
    );
    let (base, _rt) = sb.serve("[acp]\nauto_detect = true").await;
    let (status, report) = diagnose(&base, "claude").await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["probe"]["kind"], "missing_dependency", "{report}");
    let f =
        finding(&report, "node_missing").unwrap_or_else(|| panic!("应命中 node_missing：{report}"));
    assert!(
        f["title"].as_str().is_some_and(|t| t.contains("Node")),
        "{f}"
    );
    assert!(
        f["steps"].as_array().is_some_and(|s| !s.is_empty()),
        "应给出修复步骤：{f}"
    );
}

/// RT-AGDIAG-02：服务的 PATH 找不到、登录 shell 能找到 → PATH 不一致
#[tokio::test]
async fn rt_agdiag_02_path_mismatch() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("02", "{login}:/usr/bin");
    let real = sb.root.join("login").join("claude-agent-acp");
    write_exec(&real, "exit 0");
    let (base, _rt) = sb.serve("[acp]\nauto_detect = true").await;
    let (status, report) = diagnose(&base, "claude").await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(
        check(&report, "command_in_service_path").unwrap()["status"],
        "problem"
    );
    assert_eq!(
        check(&report, "command_in_login_shell").unwrap()["status"],
        "ok"
    );
    let f = finding(&report, "path_mismatch")
        .unwrap_or_else(|| panic!("应命中 path_mismatch：{report}"));
    assert!(
        f["explain"]
            .as_str()
            .is_some_and(|e| e.contains(&real.display().to_string())),
        "解释里应给出终端里找到的完整路径：{f}"
    );
}

/// RT-AGDIAG-03：只对需要 Node 的助手查版本；版本太低命中规则；node 卡住不拖住排查
#[tokio::test]
async fn rt_agdiag_03_node_version_checks() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("03", "/usr/bin");
    let marker = sb.root.join("node-calls");
    sb.bin(
        "node",
        &format!("echo x >> '{}'\nprintf 'v18.19.0\\n'", marker.display()),
    );
    sb.bin("zcode-acp-server", "exit 1");
    sb.bin("cursor-agent", "exit 1");
    let (base, _rt) = sb.serve("[acp]\nauto_detect = true").await;

    let (_, report) = diagnose(&base, "zcode").await;
    assert!(
        finding(&report, "node_too_old").is_some(),
        "ZCode 要求 Node ≥ 22：{report}"
    );
    let calls = || {
        std::fs::read_to_string(&marker)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let before = calls();
    assert!(before >= 1, "排查 ZCode 时应运行 node --version");
    let (_, report) = diagnose(&base, "cursor").await;
    assert_eq!(calls(), before, "Cursor 没有 Node 要求，不应运行 node");
    assert!(
        check(&report, "node_version").is_none_or(|c| c["status"] == "skipped"),
        "{report}"
    );

    // node 卡住：5 秒内结束并记为检查失败
    sb.bin("node", "sleep 30");
    let started = Instant::now();
    let (status, report) = diagnose(&base, "zcode").await;
    assert_eq!(status, 200, "{report}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "排查不应被卡住"
    );
    assert_eq!(
        check(&report, "node_version").unwrap()["status"],
        "error",
        "{report}"
    );
}

/// RT-AGDIAG-04：凭证文件只看是否存在，不读内容
#[tokio::test]
async fn rt_agdiag_04_credential_file_existence_only() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("04", "/usr/bin");
    sb.bin("zcode-acp-server", "exit 1");
    sb.bin("node", "printf 'v22.3.0\\n'");
    let (base, _rt) = sb.serve("[acp]\nauto_detect = true").await;

    let (_, report) = diagnose(&base, "zcode").await;
    assert_eq!(
        check(&report, "credential_file").unwrap()["status"],
        "problem",
        "{report}"
    );
    assert!(finding(&report, "credential_missing").is_some(), "{report}");

    let cred = sb.root.join("home/.zcode/v2/config.json");
    std::fs::create_dir_all(cred.parent().unwrap()).unwrap();
    std::fs::write(&cred, "{\"secret\":\"do-not-read\"}").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cred, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let (_, report) = diagnose(&base, "zcode").await;
    assert_eq!(
        check(&report, "credential_file").unwrap()["status"],
        "ok",
        "{report}"
    );
    assert!(finding(&report, "credential_missing").is_none(), "{report}");
    assert!(
        !report.to_string().contains("do-not-read"),
        "不能读取凭证内容"
    );
}

/// RT-AGDIAG-06：未知报错 → 未识别；自定义命令不做登录 shell 检查
#[tokio::test]
async fn rt_agdiag_06_unmatched_and_custom_skips_shell() {
    let _g = ENV_LOCK.lock().await;
    let sb = Sandbox::new("06", "/usr/bin");
    let weird = sb.bin("weird-agent", "echo 'something odd happened' >&2\nexit 3");
    let extra = format!(
        "[acp]\nauto_detect = false\n[[acp.agents]]\nid = \"weird\"\ncommand = \"{}\"\nrole = \"coding\"\n",
        weird.display()
    );
    let (base, _rt) = sb.serve(&extra).await;
    let (status, report) = diagnose(&base, "weird").await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["unmatched"], true, "{report}");
    assert!(
        report["findings"].as_array().is_some_and(|f| f.is_empty()),
        "{report}"
    );
    assert!(
        check(&report, "command_in_login_shell").is_none_or(|c| c["status"] == "skipped"),
        "自定义命令不做登录 shell 检查：{report}"
    );
}
