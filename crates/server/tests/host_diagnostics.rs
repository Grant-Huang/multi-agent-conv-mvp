//! Real binary HTTP/JSON-log regression; no model, browser, or remote node.
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    child: Child,
    dir: PathBuf,
    port: u16,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Server {
    async fn start(mom: bool) -> Self {
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserve.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(an_core::new_id("host-diagnostics"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cmd = if mom {
            Command::new(env!("CARGO_BIN_EXE_an-mom"))
        } else {
            Command::new(env!("CARGO_BIN_EXE_agentnexus"))
        };
        if mom {
            cmd.args(["--bind", &format!("127.0.0.1:{port}"), "--db", "mom.db"]);
        } else {
            std::fs::write(dir.join("config.toml"), format!("[server]\nbind='127.0.0.1:{port}'\ndb_path='app.db'\n[llm]\nbase_url='http://127.0.0.1:1/v1'\nmodel='test'\ntimeout_secs=1\nmax_retries=0\n[nodes]\ndiscovery=false\n[acp]\nauto_detect=false\n[logging]\ndir='logs'\n")).unwrap();
            cmd.arg("config.toml");
        }
        drop(reserve);
        let child = cmd
            .current_dir(&dir)
            .env_remove("RUST_LOG")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut s = Self { child, dir, port };
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                assert!(s.child.try_wait().unwrap().is_none(), "服务必须正常启动");
                if tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        s
    }
    async fn raw(&self, path: &str, host: &str) -> String {
        let body = json!({"title":"body-secret-should-not-log"}).to_string();
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1",self.port)).await.unwrap();
            stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer header-secret-should-not-log\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            let mut response = Vec::new();stream.read_to_end(&mut response).await.unwrap();
            String::from_utf8(response).unwrap()
        }).await.unwrap()
    }
    async fn get(&self, path: &str) -> Value {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://127.0.0.1:{}{path}", self.port))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn rejected_logs(&self) -> Vec<Value> {
        // Bounded flush window for the nonblocking JSON writer; return empty on old code.
        for _ in 0..100 {
            let records: Vec<Value> = std::fs::read_dir(self.dir.join("logs"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
                .flat_map(|text| {
                    text.lines()
                        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                        .collect::<Vec<_>>()
                })
                .filter(|v| v["event"] == "http.host_rejected")
                .collect();
            if !records.is_empty() {
                return records;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        vec![]
    }
}
fn assert_fields(fields: &Value, host: &str, path: &str) {
    assert_eq!(fields["method"], "POST");
    assert_eq!(fields["path"], path);
    assert_eq!(fields["host"], host.chars().take(100).collect::<String>());
    assert!(fields["host"].as_str().unwrap().chars().count() <= 100);
    assert_eq!(fields["task_id"], ""); // Refused before a task exists.
    let text = fields.to_string();
    for secret in [
        "body-secret-should-not-log",
        "header-secret-should-not-log",
        "query-secret-should-not-log",
    ] {
        assert!(
            !text.contains(secret),
            "日志不得包含请求体、其他请求头或查询串"
        );
    }
}

#[tokio::test]
async fn rt_host_03_rejection_is_visible_in_diagnostics_and_json_without_secrets() {
    let s = Server::start(false).await;
    let host = format!("evil.example.{}", "x".repeat(130));
    let out = s
        .raw(
            "/api/conversations?token=query-secret-should-not-log",
            &host,
        )
        .await;
    assert!(out.starts_with("HTTP/1.1 403"));
    assert!(out.contains("E_HTTP_HOST"));
    let diag = s.get("/api/diagnostics").await;
    let records: Vec<_> = diag["recent_problems"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["event"] == "http.host_rejected")
        .collect();
    assert_eq!(records.len(), 1, "Host拒绝必须进入最近问题诊断：{diag}");
    assert_eq!(records[0]["code"], "E_HTTP_HOST");
    assert_eq!(records[0]["level"], "WARN");
    assert_fields(&records[0]["fields"], &host, "/api/conversations");
    assert_eq!(diag["problem_counts"]["E_HTTP_HOST"], 1);
    assert!(
        s.get("/api/conversations")
            .await
            .as_array()
            .unwrap()
            .is_empty(),
        "拒绝请求不能创建记录"
    );
    let logs = s.rejected_logs().await;
    assert_eq!(logs.len(), 1, "拒绝事件必须写入JSON日志");
    assert_eq!(logs[0]["code"], "E_HTTP_HOST");
    assert_fields(&logs[0], &host, "/api/conversations");
    assert_eq!(
        s.get("/api/diagnostics").await["problem_counts"]["E_HTTP_HOST"],
        1,
        "合法请求不能记为Host拒绝"
    );
}

#[tokio::test]
async fn rt_host_04_standalone_mom_records_warn_json_without_changing_admission() {
    let s = Server::start(true).await;
    let host = format!("evil.example.{}", "y".repeat(130));
    assert!(s
        .raw(
            "/api/mom/scenarios/s1?token=query-secret-should-not-log",
            &host
        )
        .await
        .starts_with("HTTP/1.1 403"));
    let logs = s.rejected_logs().await;
    assert_eq!(logs.len(), 1, "独立MOM拒绝事件必须有日志订阅器并写入JSON");
    assert_eq!(logs[0]["code"], "E_HTTP_HOST");
    assert_eq!(logs[0]["level"], "WARN");
    assert_fields(&logs[0], &host, "/api/mom/scenarios/s1");
    assert!(s.get("/api/mom/cases").await["cases"]
        .as_array()
        .unwrap()
        .is_empty());
}
