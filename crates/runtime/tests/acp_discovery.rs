//! 运行时集成用例 RT-ACPD-01 ~ RT-ACPD-05（定义见 docs/testing/CASES.md「ACP 自动发现预设」）：
//! 新增的 claude / codex-acp / cursor / zcode 预设在本机"已安装"时被自动发现并注册。
//!
//! 做法：在临时目录放假的可执行文件（只是空脚本，发现阶段只检查文件、不运行），
//! 把 PATH 和 HOME 指向临时目录后构造 Runtime，再读注册表里的卡片。
//!
//! 注意：PATH / HOME 是进程级全局变量，本文件的用例用同一把锁串行执行，
//! 互不干扰；本文件是独立的测试二进制，不影响其他测试文件。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use an_agents::acp::presets;
use an_agents::util::resolve_command;
use an_core::{AgentCard, Role};
use an_runtime::config::AppConfig;
use an_runtime::store::Store;
use an_runtime::Runtime;

/// 串行化修改环境变量的用例
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 新增的 4 个预设：(卡片 id, 命令名)
const NEW_PRESETS: &[(&str, &str)] = &[
    ("claude", "claude-agent-acp"),
    ("codex-acp", "codex-acp"),
    ("cursor", "cursor-agent"),
    ("zcode", "zcode-acp-server"),
];

/// 一个用例独享的临时目录：bin/ 放假命令，home/ 当作 HOME（让 ~ 开头的额外查找目录为空）
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "an-acpd-{name}-{}-{}",
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

    fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// 在 bin/ 下放一个可执行的空脚本，冒充已安装的命令
    fn install(&self, command: &str) -> PathBuf {
        let p = self.bin().join(command);
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    /// 把 PATH 指向 bin/、HOME 指向 home/，然后用给定配置构造 Runtime，返回全部卡片。
    /// 结束后恢复原来的环境变量。
    fn cards(&self, extra_config: &str) -> Vec<AgentCard> {
        let old_path = std::env::var_os("PATH");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("PATH", self.bin());
        std::env::set_var("HOME", self.root.join("home"));
        let result = build_runtime(&self.root, extra_config);
        restore("PATH", old_path);
        restore("HOME", old_home);
        result.expect("构造 Runtime 不应报错").registry.cards()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn restore(key: &str, old: Option<std::ffi::OsString>) {
    match old {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

/// 最小配置：模型指向不可连接的地址（构造时不会调用模型），开启自动发现，
/// 关闭内置 Codex，工作目录放在临时目录里。
fn build_runtime(root: &Path, extra_config: &str) -> anyhow::Result<Runtime> {
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
[acp]
auto_detect = true
{extra_config}
"#,
        files = root.join("files").display(),
        code = root.join("code").display(),
    );
    let (config, _unknown) = AppConfig::parse(&text)?;
    Runtime::new(config, Store::open(Path::new(":memory:"))?)
}

fn find<'a>(cards: &'a [AgentCard], id: &str) -> Option<&'a AgentCard> {
    cards.iter().find(|c| c.id == id)
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// RT-ACPD-01：4 个新预设的命令都已安装 → 4 张卡片，属性正确
#[test]
fn rt_acpd_01_installed_presets_are_registered() {
    let _g = lock();
    let sb = Sandbox::new("01");
    for (_, cmd) in NEW_PRESETS {
        sb.install(cmd);
    }
    let cards = sb.cards("");
    for (id, cmd) in NEW_PRESETS {
        let c =
            find(&cards, id).unwrap_or_else(|| panic!("已安装 {cmd}，但没有自动发现 agent `{id}`"));
        assert_eq!(c.kind, "acp", "{id} 应以 ACP 方式接入");
        assert_eq!(c.role, Role::Coding, "{id} 的角色应为 coding");
        assert_eq!(c.agent_type, *id, "{id} 的类型应等于 id");
        assert!(c.enabled, "{id} 已安装，应为可用");
    }
}

/// RT-ACPD-02：只有一个无关的 `agent` 命令 → 不能误认成 Cursor
#[test]
fn rt_acpd_02_generic_agent_command_is_not_cursor() {
    let _g = lock();
    let sb = Sandbox::new("02");
    sb.install("agent");
    let cards = sb.cards("");
    assert!(
        find(&cards, "cursor").is_none(),
        "PATH 里只有通用名 `agent` 时不应识别为 Cursor"
    );
}

/// RT-ACPD-03：用户自定义了同 id 的 agent → 以用户配置为准，且只有一张卡片
#[test]
fn rt_acpd_03_user_config_overrides_preset() {
    let _g = lock();
    let sb = Sandbox::new("03");
    sb.install("claude-agent-acp");
    let mine = sb.install("my-claude-wrapper");
    let cards = sb.cards(&format!(
        r#"
[[acp.agents]]
id = "claude"
name = "我的 Claude"
command = "{}"
role = "general"
"#,
        mine.display()
    ));
    let same_id: Vec<_> = cards.iter().filter(|c| c.id == "claude").collect();
    assert_eq!(same_id.len(), 1, "同一个 id 只能有一张卡片");
    assert_eq!(same_id[0].name, "我的 Claude", "名称应取用户配置");
    assert_eq!(same_id[0].role, Role::General, "角色应取用户配置");
}

/// RT-ACPD-04：都没安装 → 不报错；卡片出现与否与"命令能否被找到"一致。
/// /usr/local/bin、/opt/homebrew/bin 等系统目录无法在测试里隔离，
/// 所以按同样的查找规则核对：找不到的命令绝不能出现卡片，找得到的必须出现。
#[test]
fn rt_acpd_04_missing_commands_are_not_registered() {
    let _g = lock();
    let sb = Sandbox::new("04");
    let old_path = std::env::var_os("PATH");
    let old_home = std::env::var_os("HOME");
    std::env::set_var("PATH", sb.bin());
    std::env::set_var("HOME", sb.root.join("home"));
    let found: Vec<bool> = NEW_PRESETS
        .iter()
        .map(|(_, cmd)| resolve_command(cmd).is_some())
        .collect();
    restore("PATH", old_path);
    restore("HOME", old_home);

    let cards = sb.cards("");
    for ((id, cmd), installed) in NEW_PRESETS.iter().zip(found) {
        assert_eq!(
            find(&cards, id).is_some(),
            installed,
            "{id}：命令 {cmd} {}，卡片应{}",
            if installed {
                "在系统目录中存在"
            } else {
                "找不到"
            },
            if installed { "出现" } else { "不出现" }
        );
    }
}

/// RT-ACPD-05：原有 3 个预设的顺序、命令、参数不变；新旧预设可同时注册
#[test]
fn rt_acpd_05_existing_presets_unchanged() {
    let p = presets();
    let head: Vec<(&str, &str, Vec<&str>)> = p
        .iter()
        .take(3)
        .map(|c| {
            (
                c.id.as_str(),
                c.command.as_str(),
                c.args.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        head,
        vec![
            ("opencode", "opencode", vec!["acp"]),
            ("hermes", "hermes", vec!["acp"]),
            ("pi", "pi-acp", vec![]),
        ],
        "原有预设的顺序、命令、参数不应改变"
    );
    let ids: Vec<&str> = p.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids.iter().skip(3).copied().collect::<Vec<_>>(),
        vec!["claude", "codex-acp", "cursor", "zcode"],
        "新预设应排在原有预设之后"
    );

    let _g = lock();
    let sb = Sandbox::new("05");
    sb.install("opencode");
    sb.install("claude-agent-acp");
    let cards = sb.cards("");
    assert!(find(&cards, "opencode").is_some(), "opencode 应仍被发现");
    assert!(find(&cards, "claude").is_some(), "claude 应被发现");
}
