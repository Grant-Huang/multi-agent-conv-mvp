//! Regression for PR #70 pre-review: logging must not prevent startup or amplify paths.
use serde_json::Value;
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
    async fn start(mom: bool, logs_file: bool) -> Self {
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserve.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(an_core::new_id("host-log-limits"));
        std::fs::create_dir_all(&dir).unwrap();
        if logs_file {
            std::fs::write(dir.join("logs"), "preserve existing file").unwrap();
        }
        let output = std::fs::File::create(dir.join("console.txt")).unwrap();
        let mut command = if mom {
            Command::new(env!("CARGO_BIN_EXE_an-mom"))
        } else {
            Command::new(env!("CARGO_BIN_EXE_agentnexus"))
        };
        if mom {
            command.args(["--bind", &format!("127.0.0.1:{port}"), "--db", "mom.db"]);
        } else {
            std::fs::write(dir.join("config.toml"),format!("[server]\nbind='127.0.0.1:{port}'\ndb_path='app.db'\n[llm]\nbase_url='http://127.0.0.1:1/v1'\nmodel='test'\ntimeout_secs=1\nmax_retries=0\n[nodes]\ndiscovery=false\n[acp]\nauto_detect=false\n[logging]\ndir='logs'\n")).unwrap();
            command.arg("config.toml");
        }
        drop(reserve);
        let child = command
            .current_dir(&dir)
            .env_remove("RUST_LOG")
            .env("NO_COLOR", "1")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        let mut s = Self { child, dir, port };
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let exited = s.child.try_wait().unwrap();
                assert!(
                    exited.is_none(),
                    "日志不可用时服务必须继续启动，不能panic：{exited:?}; {}",
                    std::fs::read_to_string(s.dir.join("console.txt")).unwrap()
                );
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
    async fn reject(&self, path: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!(
                        "GET {path} HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut result = vec![];
            stream.read_to_end(&mut result).await.unwrap();
            String::from_utf8(result).unwrap()
        })
        .await
        .unwrap()
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
    async fn rejection_log(&self) -> Value {
        for _ in 0..100 {
            for entry in std::fs::read_dir(self.dir.join("logs"))
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    for line in text.lines() {
                        if let Ok(record) = serde_json::from_str::<Value>(line) {
                            if record["event"] == "http.host_rejected" {
                                return record;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("Host拒绝必须有JSON日志");
    }
}
async fn check_console_fallback(mom: bool) {
    let mut s = Server::start(mom, true).await;
    let path = if mom {
        "/api/mom/cases"
    } else {
        "/api/conversations"
    };
    let _ = s.get(path).await;
    assert!(s.reject("/blocked").await.starts_with("HTTP/1.1 403"));
    assert!(s.child.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(s.dir.join("logs")).unwrap(),
        "preserve existing file"
    );
    let console = std::fs::read_to_string(s.dir.join("console.txt")).unwrap();
    assert!(
        console.contains("log.file_unavailable"),
        "文件日志降级必须有提示：{console}"
    );
    assert!(
        console.contains("http.host_rejected") && console.contains("E_HTTP_HOST"),
        "降级后Host拒绝必须继续输出到控制台：{console}"
    );
    if !mom {
        let d = s.get("/api/diagnostics").await;
        assert_eq!(
            d["problem_counts"]["E_HTTP_HOST"], 1,
            "降级后内存诊断仍应工作"
        );
    }
}
#[tokio::test]
async fn rt_host_05_mom_starts_with_console_logging_when_log_directory_is_a_file() {
    check_console_fallback(true).await;
}
#[tokio::test]
async fn rt_host_07_main_service_uses_same_non_panicking_log_fallback() {
    check_console_fallback(false).await;
}
#[tokio::test]
async fn rt_host_06_large_rejected_path_is_bounded_in_json_and_diagnostics() {
    for mom in [true, false] {
        let s = Server::start(mom, false).await;
        let path = format!("/{}", "a".repeat(60_000));
        assert!(s.reject(&path).await.starts_with("HTTP/1.1 403"));
        let record = s.rejection_log().await;
        let logged = record["path"].as_str().unwrap();
        assert!(
            logged.chars().count() <= 200,
            "拒绝日志path必须限长：实际{}字符",
            logged.chars().count()
        );
        assert_eq!(logged, path.chars().take(200).collect::<String>());
        assert!(
            record.to_string().len() < 2000,
            "拒绝日志不得随60KB输入膨胀"
        );
        assert_eq!(record["code"], "E_HTTP_HOST");
        if !mom {
            let d = s.get("/api/diagnostics").await;
            let events: Vec<_> = d["recent_problems"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["event"] == "http.host_rejected")
                .collect();
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0]["fields"]["path"], logged,
                "诊断也必须使用同一截断摘要"
            );
        }
    }
}
