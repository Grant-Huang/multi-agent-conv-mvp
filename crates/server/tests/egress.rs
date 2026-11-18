//! 外发守卫的启动级测试：`[egress]` 配置解析、启动校验、DLP 工具不暴露给模型。
use an_runtime::{config::AppConfig, store::Store, Runtime};
use std::path::Path;

fn fixture() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../agents/tests/fixtures/dlp_server.py")
        .to_string_lossy()
        .into_owned()
}

/// mode / LLM 地址 / 工具绑定 / LLM 绑定 可变；websearch 命令用 python3，使 web_search、web_fetch 被注册
fn config(mode: &str, llm_url: &str, tool_bindings: &[&str], llm_bound: bool) -> String {
    let dir = std::env::temp_dir().join(an_core::new_id("egress-rt"));
    let log = dir.join("dlp.jsonl");
    let mut text = format!(
        r#"
[llm]
base_url = "{llm_url}"
model = "mock"
[agents.general]
workspace_dir = {ws:?}
websearch_command = "python3"
[agents.coding]
enabled = false
[nodes]
discovery = false
[egress]
mode = "{mode}"
timeout_ms = 5000
dlp_command = "python3"
dlp_args = [{fx:?}, {log:?}]
"#,
        ws = dir.join("ws").to_string_lossy(),
        fx = fixture(),
        log = log.to_string_lossy(),
    );
    for t in tool_bindings {
        text.push_str(&format!(
            "[[egress.tools]]\ntool = \"{t}\"\ndestination_id = \"public-search\"\nchannel = \"external_api\"\npurpose = \"web_research\"\n"
        ));
    }
    if llm_bound {
        text.push_str("[[egress.llm]]\nbackend = \"default\"\ndestination_id = \"cloud-llm\"\npurpose = \"llm_inference\"\n");
    }
    text
}

async fn start(text: &str) -> anyhow::Result<Runtime> {
    let (cfg, unknown) = AppConfig::parse(text)?;
    assert!(unknown.is_empty(), "{unknown:?}");
    Runtime::with_mcp_diagnostics(
        cfg,
        Store::open(Path::new(":memory:")).unwrap(),
        an_observe::diagnostics_only(),
    )
    .await
}

#[tokio::test]
async fn rt_egr_11_enforce_requires_every_network_tool_to_be_bound() {
    let err = start(&config(
        "enforce",
        "http://127.0.0.1:1/v1",
        &["web_search"],
        false,
    ))
    .await
    .err()
    .expect("缺少 web_fetch 绑定时应拒绝启动")
    .to_string();
    assert!(err.contains("web_fetch"), "{err}");
}

#[tokio::test]
async fn rt_egr_12_enforce_starts_when_bound_and_never_exposes_dlp_tools() {
    let rt = start(&config(
        "enforce",
        "http://127.0.0.1:1/v1",
        &["web_search", "web_fetch"],
        false,
    ))
    .await
    .unwrap();
    assert!(
        !rt.capabilities.iter().any(|c| c.tool_id.contains("egress")),
        "DLP 工具不得出现在模型可见的工具清单中"
    );
}

#[tokio::test]
async fn rt_egr_13_cloud_llm_must_be_bound_in_enforce_but_only_warns_in_audit() {
    let cloud = "https://api.example.invalid/v1";
    let tools = ["web_search", "web_fetch"];
    let err = start(&config("enforce", cloud, &tools, false))
        .await
        .err()
        .expect("云端后端未登记时 enforce 应拒绝启动")
        .to_string();
    assert!(err.contains("default"), "{err}");
    assert!(start(&config("audit", cloud, &tools, false)).await.is_ok());
    assert!(start(&config("enforce", cloud, &tools, true)).await.is_ok());
}

#[test]
fn rt_egr_14_egress_is_off_by_default_and_sync_constructor_refuses_it() {
    let base = "[llm]\nbase_url = \"http://127.0.0.1:1/v1\"\nmodel = \"m\"\n[nodes]\ndiscovery = false\n[agents.coding]\nenabled = false\n";
    let (cfg, _) = AppConfig::parse(base).unwrap();
    assert_eq!(cfg.egress.mode, an_agents::egress::EgressMode::Off);
    let (cfg, _) = AppConfig::parse(&format!(
        "{base}[egress]\nmode = \"audit\"\ndlp_command = \"python3\"\n"
    ))
    .unwrap();
    let err = Runtime::new(cfg, Store::open(Path::new(":memory:")).unwrap())
        .err()
        .expect("同步构造不能启用外发检查")
        .to_string();
    assert!(err.contains("egress"), "{err}");
}
