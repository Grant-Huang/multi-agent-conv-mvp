//! RT-AGMGMT-01（定义见 docs/testing/CASES.md）：
//! 管理助手 / 改配置类任务目标被 Router 拒绝，不派 General 读配置。

use std::collections::BTreeMap;

use an_core::Risk;
use an_llm::LlmConfig;
use an_runtime::policy::Policy;
use an_runtime::registry::AgentRegistry;
use an_runtime::router::Router;

fn router() -> Router {
    let llm = an_llm::LlmClient::new(LlmConfig {
        base_url: "http://127.0.0.1:9/v1".into(),
        api_key: String::new(),
        api_key_env: None,
        model: "m".into(),
        temperature: 0.2,
        max_tokens: None,
        timeout_secs: 1,
        structured_mode: Default::default(),
        max_retries: 0,
    })
    .unwrap();
    Router::new(
        llm,
        AgentRegistry::new(BTreeMap::new()),
        ["read_file", "write_file", "list_dir"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        Policy {
            approval_risk: Risk::High,
        },
    )
}

#[tokio::test]
async fn rt_agmgmt_01_rejects_agent_management_goals() {
    let r = router();
    for goal in [
        "管理助手",
        "查看并修改 agent 配置",
        "读 config.toml 把 Cursor 接进来",
        "接入一个写代码的助手",
    ] {
        let err = match r.route(goal, "", &[]).await {
            Err(e) => e,
            Ok(_) => panic!("应拒绝：{goal}"),
        };
        assert!(
            err.message.contains("助手") || err.message.contains("manage"),
            "错误应说明走管理助手路径：{} → {}",
            goal,
            err.message
        );
        assert!(
            err.message.contains("配置")
                || err.message.contains("页签")
                || err.message.contains("General")
                || err.message.contains("读"),
            "错误应提示不要派 General 读配置：{}",
            err.message
        );
    }
    let work = match r.route("用计算器算 1+1", "", &[]).await {
        Err(e) => e,
        Ok(_) => panic!("无模型时应失败"),
    };
    assert!(
        !work.message.contains("管理助手") && !work.message.contains("页签"),
        "普通任务不应被管理助手规则拦下：{}",
        work.message
    );
}
