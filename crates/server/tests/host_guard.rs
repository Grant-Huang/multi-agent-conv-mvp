//! Real production entry points, including static fallback and nested MOM.
#[path = "support/service_readiness.rs"]
mod service_readiness;
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
        let _startup = service_readiness::startup_guard().await;
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserve.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(an_core::new_id("host-guard"));
        std::fs::create_dir_all(dir.join("static")).unwrap();
        std::fs::write(dir.join("static/index.html"), "guarded static").unwrap();
        let mut cmd = if mom {
            Command::new(env!("CARGO_BIN_EXE_an-mom"))
        } else {
            Command::new(env!("CARGO_BIN_EXE_agentnexus"))
        };
        if mom {
            cmd.args(["--bind", &format!("127.0.0.1:{port}"), "--db", "mom.db"]);
        } else {
            // No real model or remote node; immediate local connection refusal on model probe.
            std::fs::write(dir.join("config.toml"),format!("[server]\nbind='127.0.0.1:{port}'\ndb_path='app.db'\nstatic_dir='static'\n[llm]\nbase_url='http://127.0.0.1:1/v1'\nmodel='test'\ntimeout_secs=1\nmax_retries=0\n[nodes]\ndiscovery=false\n[logging]\ndir=''\n")).unwrap();
            cmd.arg("config.toml");
        }
        let output = std::fs::File::create(dir.join("console.txt")).unwrap();
        drop(reserve);
        let child = cmd
            .current_dir(&dir)
            .env_remove("RUST_LOG")
            .env("NO_COLOR", "1")
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap();
        let mut s = Self { child, dir, port };
        service_readiness::wait_for_service(
            &mut s.child,
            &s.dir.join("console.txt"),
            port,
            mom,
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        s
    }
    async fn request(&self, method: &str, path: &str, headers: &str, body: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5),async {
            let mut stream=tokio::net::TcpStream::connect(("127.0.0.1",self.port)).await.unwrap();
            stream.write_all(format!("{method} {path} HTTP/1.1\r\n{headers}Connection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            let mut out=Vec::new(); stream.read_to_end(&mut out).await.unwrap();
            String::from_utf8(out).unwrap()
        }).await.unwrap()
    }
}

#[tokio::test]
async fn rt_host_01_production_entrypoints_reject_foreign_host_before_side_effects() {
    for mom in [false, true] {
        let s = Server::start(mom).await;
        let (list, create) = if mom {
            ("/api/mom/cases", "/api/mom/scenarios/s1")
        } else {
            ("/api/conversations", "/api/conversations")
        };
        let good = format!("Host: 127.0.0.1:{}\r\n", s.port);
        let before = s.request("GET", list, &good, "").await;
        assert!(before.starts_with("HTTP/1.1 200"), "{before}");
        let bad=format!("Host: attacker.invalid:{}\r\nX-Forwarded-Host: localhost:{}\r\nForwarded: host=localhost:{}\r\n",s.port,s.port,s.port);
        let out = s.request("POST", create, &bad, "{}").await;
        assert!(
            out.starts_with("HTTP/1.1 403"),
            "非法 Host 必须在写入前被拒绝：{out}"
        );
        let after = s.request("GET", list, &good, "").await;
        assert_eq!(
            before.split("\r\n\r\n").last(),
            after.split("\r\n\r\n").last(),
            "被拒绝的请求不得创建业务记录"
        );
        for path in [
            "/",
            list,
            if mom { "/api/mom/gaps" } else { "/api/events" },
            if mom { "/api/mom/gaps" } else { "/mom/" },
        ] {
            let out = s.request("GET", path, &bad, "").await;
            assert!(
                out.starts_with("HTTP/1.1 403"),
                "所有页面/API/SSE 必须受保护：{path}: {out}"
            );
        }
    }
}

#[tokio::test]
async fn rt_host_02_strict_authority_and_real_listener_port() {
    let s = Server::start(true).await;
    for host in [
        format!("localhost:{}", s.port),
        format!("127.0.0.1:{}", s.port),
        format!("[::1]:{}", s.port),
    ] {
        let out = s
            .request("GET", "/api/mom/cases", &format!("Host: {host}\r\n"), "")
            .await;
        assert!(out.starts_with("HTTP/1.1 200"), "{host}: {out}");
    }
    for host in [
        "localhost.attacker.invalid".to_string(),
        format!(
            "localhost:{}",
            if s.port == 65535 {
                s.port - 1
            } else {
                s.port + 1
            }
        ),
        "user@localhost:80".into(),
        "localhost:".into(),
        "127.0.0.2:80".into(),
    ] {
        let out = s
            .request("GET", "/api/mom/cases", &format!("Host: {host}\r\n"), "")
            .await;
        assert!(
            out.starts_with("HTTP/1.1 403") || out.starts_with("HTTP/1.1 400"),
            "必须拒绝非法 authority：{host}: {out}"
        );
    }
    for headers in [
        String::new(),
        format!("Host: localhost:{}\r\nHost: attacker.invalid\r\n", s.port),
    ] {
        let out = s.request("GET", "/api/mom/cases", &headers, "").await;
        assert!(
            out.starts_with("HTTP/1.1 403") || out.starts_with("HTTP/1.1 400"),
            "必须拒绝缺失/重复 Host：{out}"
        );
    }
}

#[tokio::test]
async fn rt_host_11_host_guard_rejects_foreign_listener_as_child_readiness() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = std::env::temp_dir().join(an_core::new_id("foreign-listener"));
    std::fs::create_dir_all(&dir).unwrap();
    let console = dir.join("console.txt");
    std::fs::write(&console, "").unwrap();
    // A live child that has not started a service; exec avoids leaving grandchildren.
    let mut child = Command::new("sh")
        .args(["-c", "exec sleep 5"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let result = service_readiness::wait_for_service(
        &mut child,
        &console,
        port,
        false,
        Duration::from_millis(150),
    )
    .await;
    let alive = child.try_wait().unwrap().is_none();
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(alive, "自己的子进程仍活着但尚未启动服务");
    assert!(
        result.is_err(),
        "不得因别人的监听套接字而宣布自己的服务就绪：{result:?}"
    );
}
