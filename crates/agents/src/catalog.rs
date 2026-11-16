//! Agent 目录（AG.1）：给每个内置 ACP 预设配上"给人看的说明"。
//!
//! 预设本身（命令、参数、角色）在 [`crate::acp::presets`]；这里只补充用途、安装步骤、登录方式、
//! 依赖要求和文档链接，供界面和对话里的引导卡片使用。不改 `AcpConfig`，所以不新增配置字段。
//!
//! 安装命令和依赖要求来源（2026-11-16 核实）：
//! - npm 包名与可执行文件名：`npm view <包名> bin`
//! - Cursor CLI ACP：https://cursor.com/docs/cli/acp
//! - zcode-acp-server 的 README：需要 Node ≥ 22、zcode CLI 在 PATH 上、ZCode App 生成的凭证
//! - opencode：https://opencode.ai/docs/acp ；Hermes：https://hermes-agent.nousresearch.com/docs/user-guide/features/acp
//! - 未能在当前环境核实的安装命令（opencode、Cursor CLI 的安装脚本）不写命令，只指向文档

use an_core::{InstallStep, Role};

use crate::acp::{presets, AcpConfig};

/// 一个预设的说明（不含本机状态）
#[derive(Debug, Clone)]
pub struct CatalogMeta {
    pub id: &'static str,
    /// 一句话用途
    pub purpose: &'static str,
    pub install: Vec<InstallStep>,
    pub login: Option<&'static str>,
    pub requirements: Vec<&'static str>,
    pub docs_url: Option<&'static str>,
}

/// 目录中的一项：预设配置 + 说明
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub preset: AcpConfig,
    pub meta: CatalogMeta,
}

impl CatalogEntry {
    pub fn id(&self) -> &str {
        &self.preset.id
    }
    pub fn name(&self) -> String {
        self.preset
            .name
            .clone()
            .unwrap_or_else(|| self.preset.id.clone())
    }
    pub fn role(&self) -> Role {
        self.preset.role
    }
}

/// 一条安装步骤：`command` 为空表示纯说明
fn step(platform: &str, text: &str, command: Option<&str>) -> InstallStep {
    InstallStep {
        platform: platform.into(),
        text: text.into(),
        command: command.map(String::from),
    }
}

/// 按预设 id 给出说明；没有收录的 id 返回 None（测试保证每个预设都有）
fn meta(id: &str) -> Option<CatalogMeta> {
    let m = match id {
        "opencode" => CatalogMeta {
            id: "opencode",
            purpose: "开源的编码助手，可以读写代码、运行命令",
            install: vec![step("all", "按官网说明安装 opencode（见文档链接）", None)],
            login: Some("首次使用前按 opencode 文档配置模型账号"),
            requirements: vec![],
            docs_url: Some("https://opencode.ai/docs/acp"),
        },
        "hermes" => CatalogMeta {
            id: "hermes",
            purpose: "通用助手：查资料、写文档等非编码任务",
            install: vec![step(
                "all",
                "安装 Hermes Agent 及其 ACP 扩展（见文档）",
                None,
            )],
            login: None,
            requirements: vec![],
            docs_url: Some("https://hermes-agent.nousresearch.com/docs/user-guide/features/acp"),
        },
        "pi" => CatalogMeta {
            id: "pi",
            purpose: "轻量编码助手（经第三方适配器 pi-acp 接入，成熟度较低）",
            install: vec![
                step("all", "安装 pi", None),
                step("all", "安装第三方适配器 pi-acp", None),
            ],
            login: None,
            requirements: vec![],
            docs_url: None,
        },
        "claude" => CatalogMeta {
            id: "claude",
            purpose: "Anthropic 的编码助手 Claude Code，擅长读懂和修改代码",
            install: vec![
                step(
                    "all",
                    "安装 Claude Code 并登录一次（终端运行 claude，按提示登录）",
                    None,
                ),
                step(
                    "all",
                    "安装 ACP 适配器",
                    Some("npm i -g @agentclientprotocol/claude-agent-acp"),
                ),
            ],
            login: Some("在终端运行 claude 按提示登录；或设置环境变量 ANTHROPIC_API_KEY"),
            requirements: vec!["Node.js"],
            docs_url: Some("https://www.npmjs.com/package/@agentclientprotocol/claude-agent-acp"),
        },
        "codex-acp" => CatalogMeta {
            id: "codex-acp",
            purpose: "OpenAI 的编码助手 Codex（通过 ACP 接入，过程可见）",
            install: vec![
                step("all", "安装 Codex CLI 并登录", Some("codex login")),
                step(
                    "all",
                    "安装 ACP 适配器",
                    Some("npm i -g @agentclientprotocol/codex-acp"),
                ),
            ],
            login: Some("在终端运行 codex login；或设置环境变量 OPENAI_API_KEY"),
            requirements: vec!["Node.js"],
            docs_url: Some("https://www.npmjs.com/package/@agentclientprotocol/codex-acp"),
        },
        "cursor" => CatalogMeta {
            id: "cursor",
            purpose: "Cursor 的命令行编码助手，原生支持 ACP",
            install: vec![
                step("all", "按官网说明安装 Cursor CLI（见文档链接）", None),
                step("all", "登录 Cursor 账号", Some("agent login")),
            ],
            login: Some("在终端运行 agent login"),
            requirements: vec![],
            docs_url: Some("https://cursor.com/docs/cli/acp"),
        },
        "zcode" => CatalogMeta {
            id: "zcode",
            purpose: "智谱 ZCode 编码助手（经社区桥接 zcode-acp-server 接入）",
            install: vec![
                step(
                    "all",
                    "安装 ZCode App 并登录一次（会生成 ~/.zcode 下的凭证）",
                    None,
                ),
                step("all", "确认 zcode 命令在 PATH 上（或设置 ZCODE_BIN）", None),
                step("all", "安装 ACP 桥接", Some("npm i -g zcode-acp-server")),
            ],
            login: Some("打开 ZCode App 登录一次"),
            requirements: vec!["Node.js ≥ 22"],
            docs_url: Some("https://www.npmjs.com/package/zcode-acp-server"),
        },
        _ => return None,
    };
    Some(m)
}

/// 完整目录：按预设顺序，每个预设配上说明。没有说明的预设不会出现（测试保证不存在这种情况）。
pub fn catalog() -> Vec<CatalogEntry> {
    presets()
        .into_iter()
        .filter_map(|preset| meta(&preset.id).map(|meta| CatalogEntry { preset, meta }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个预设都有说明，且至少一条安装步骤
    #[test]
    fn every_preset_has_meta() {
        let ids: Vec<String> = presets().into_iter().map(|p| p.id).collect();
        let cat = catalog();
        assert_eq!(
            cat.iter().map(|e| e.id().to_string()).collect::<Vec<_>>(),
            ids,
            "目录必须覆盖全部预设且顺序一致"
        );
        for e in &cat {
            assert_eq!(e.meta.id, e.id());
            assert!(!e.meta.purpose.is_empty());
            assert!(!e.meta.install.is_empty(), "{} 缺少安装步骤", e.id());
        }
    }
}
