//! RT-I325-02（Issue #325）：诊断矛盾规则——检测 not_installed 但体检能找到命令时，
//! 规则 installed_need_recheck 给出明确原因与下一步；未安装场景保持原规则。

use an_agents::diagnose::{rules, Checkup, LoginShell, NodeCheck, RuleContext};
use an_core::{ProbeKind, ProbeOutcome};
use std::path::PathBuf;

#[test]
fn rt_i325_02_diagnose_rule_yields_recheck() {
    let probe = ProbeOutcome {
        id: "claude".into(),
        kind: ProbeKind::NotInstalled,
        summary: "本机找不到这个 agent 的启动命令".into(),
        next_step: None,
        detail: None,
    };
    let c = Checkup {
        in_service_path: Some(PathBuf::from("/Users/x/.npm-global/bin/claude-agent-acp")),
        login_shell: LoginShell::Found(PathBuf::from("/bin/zsh")),
        node: NodeCheck::Version {
            major: 22,
            text: "v22.22.0".into(),
        },
        credentials: vec![],
        node_min_major: Some(18),
    };
    let ctx = RuleContext {
        name: "Claude Code".into(),
        command: "claude-agent-acp".into(),
        args: vec![],
        install: vec![],
        login: None,
    };
    let findings = rules(&probe, &c, &ctx);
    let hit = findings
        .iter()
        .find(|f| f.rule == "installed_need_recheck")
        .expect("应命中 installed_need_recheck 规则");
    assert!(hit.title.contains("已安装"), "{hit:?}");
    assert!(hit.steps.len() >= 2, "应给出重新检测/重启的下一步");

    // 对照：命令确实不存在（in_service_path None、终端也找不到）→ 原规则
    let c2 = Checkup {
        in_service_path: None,
        login_shell: LoginShell::NotFound,
        ..c.clone()
    };
    let findings2 = rules(&probe, &c2, &ctx);
    assert!(
        findings2.iter().any(|f| f.rule == "not_installed"),
        "未安装时仍是 not_installed 规则：{findings2:?}"
    );
    assert!(
        !findings2.iter().any(|f| f.rule == "installed_need_recheck"),
        "未安装时不得命中矛盾规则"
    );
}
