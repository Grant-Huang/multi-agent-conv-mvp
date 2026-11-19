//! 助手排查（AG.5 第 1、2 层）：本机体检 + 已知问题规则库。全部在本机完成，不外发，不调用模型。
//!
//! 本机体检只运行**写死的只读检查**，不接受用户或模型提供的任何参数：
//! - 服务进程能否找到启动命令（只查文件，不运行）；
//! - 登录 shell 能否找到启动命令：让登录 shell 打印 PATH（固定脚本），再在这份 PATH 里查找。
//!   **只对目录里的预设命令做**，用户自定义的命令不做这项，避免把用户输入拼进命令；
//! - `node --version`：只对目录里写明需要 Node 的助手运行；
//! - 凭证文件**是否存在**：只看元数据，绝不读取内容。
//!
//! 超时与清理：每条命令最多 [`CHECK_TIMEOUT`]；超时时按进程组结束（先 SIGTERM，再 SIGKILL），
//! 调用方取消（future 被丢弃）时立即 SIGKILL 整个进程组（见 `proc_group`）。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use an_core::{CheckItem, Finding, InstallStep, ProbeKind, ProbeOutcome};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::proc_group::{self, GroupChild};
use crate::util::{expand_tilde, resolve_command};

/// 单条体检命令的超时
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// 体检需要的输入（来自目录或已有配置，不来自用户的自由输入）
#[derive(Debug, Clone)]
pub struct CheckupInput {
    /// 启动命令（命令名或路径）
    pub command: String,
    /// 是否是目录里的预设命令（只有预设命令才做登录 shell 检查）
    pub is_preset: bool,
    /// 需要 Node 时的最低主版本；Some(0) 表示需要但不限版本
    pub node_min_major: Option<u32>,
    /// 凭证文件（只检查是否存在）
    pub credential_files: Vec<String>,
}

/// 登录 shell 检查的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginShell {
    Skipped,
    Found(PathBuf),
    NotFound,
    Error(String),
}

/// node 版本检查的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeCheck {
    Skipped,
    Version { major: u32, text: String },
    NotFound,
    Error(String),
}

/// 体检结果
#[derive(Debug, Clone)]
pub struct Checkup {
    pub in_service_path: Option<PathBuf>,
    pub login_shell: LoginShell,
    pub node: NodeCheck,
    /// (凭证文件, 是否存在)
    pub credentials: Vec<(String, bool)>,
    pub node_min_major: Option<u32>,
}

/// 运行一条写死的命令，返回标准输出。超时或失败返回大白话原因
async fn run_fixed(program: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    proc_group::configure(&mut cmd);
    let mut child = GroupChild::spawn(&mut cmd, "").map_err(|e| format!("无法运行：{e}"))?;
    let mut stdout = child.child_mut().stdout.take();
    let work = async {
        let mut out = String::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_string(&mut out).await;
        }
        let status = child.wait().await;
        (out, status)
    };
    match tokio::time::timeout(CHECK_TIMEOUT, work).await {
        Ok((out, Ok(status))) => {
            child.kill_leftovers();
            if status.success() {
                Ok(out)
            } else {
                Err(format!("退出码 {}", status.code().unwrap_or(-1)))
            }
        }
        Ok((_, Err(e))) => Err(format!("运行失败：{e}")),
        Err(_) => {
            child.terminate("timeout").await;
            Err(format!("{} 秒内没有结果，已结束", CHECK_TIMEOUT.as_secs()))
        }
    }
}

/// 可执行文件判断（与 util::resolve_command 相同的规则）
fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// 在一份 PATH 字符串里查找命令（不经过 shell）
fn find_in_path(path: &str, command: &str) -> Option<PathBuf> {
    if command.contains('/') {
        let p = PathBuf::from(expand_tilde(command));
        return is_executable(&p).then_some(p);
    }
    std::env::split_paths(path)
        .map(|d| d.join(command))
        .find(|p| is_executable(p))
}

/// 登录 shell 的 PATH：用固定脚本让登录 shell 打印 PATH，不拼接任何外部输入
async fn login_shell_path() -> Result<String, String> {
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    run_fixed(&shell, &["-lc", "printf '%s' \"$PATH\""])
        .await
        .map(|s| s.trim().to_string())
}

/// 从 `node --version` 的输出解析主版本（例如 v22.3.0 → 22）
fn parse_node_major(text: &str) -> Option<u32> {
    text.trim()
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// 本机体检
pub async fn checkup(input: &CheckupInput) -> Checkup {
    let in_service_path = resolve_command(&input.command);
    // 登录 shell 的 PATH 只在需要时取一次（预设命令或需要找 node 时）
    let need_login = input.is_preset || input.node_min_major.is_some();
    let login_path = if need_login {
        Some(login_shell_path().await)
    } else {
        None
    };
    let login_shell = match (&login_path, input.is_preset) {
        (_, false) | (None, _) => LoginShell::Skipped,
        (Some(Ok(p)), true) => match find_in_path(p, &input.command) {
            Some(found) => LoginShell::Found(found),
            None => LoginShell::NotFound,
        },
        (Some(Err(e)), true) => LoginShell::Error(e.clone()),
    };
    let node = match input.node_min_major {
        None => NodeCheck::Skipped,
        Some(_) => {
            let node_bin = resolve_command("node").or_else(|| match &login_path {
                Some(Ok(p)) => find_in_path(p, "node"),
                _ => None,
            });
            match node_bin {
                None => NodeCheck::NotFound,
                Some(bin) => match run_fixed(&bin, &["--version"]).await {
                    Ok(out) => match parse_node_major(&out) {
                        Some(major) => NodeCheck::Version {
                            major,
                            text: out.trim().to_string(),
                        },
                        None => NodeCheck::Error(format!("看不懂 node 的版本输出：{}", out.trim())),
                    },
                    Err(e) => NodeCheck::Error(e),
                },
            }
        }
    };
    let credentials = input
        .credential_files
        .iter()
        // 只看是否存在（symlink_metadata 只读元数据，不打开文件）
        .map(|f| {
            (
                f.clone(),
                std::fs::symlink_metadata(expand_tilde(f)).is_ok(),
            )
        })
        .collect();
    Checkup {
        in_service_path,
        login_shell,
        node,
        credentials,
        node_min_major: input.node_min_major,
    }
}

/// 体检结果 → 给人看的检查项
pub fn check_items(c: &Checkup) -> Vec<CheckItem> {
    let item = |key: &str, name: &str, status: &str, detail: String| CheckItem {
        key: key.into(),
        name: name.into(),
        status: status.into(),
        detail,
    };
    let mut v = vec![match &c.in_service_path {
        Some(p) => item(
            "command_in_service_path",
            "AgentNexus 能找到启动命令",
            "ok",
            format!("找到了：{}", p.display()),
        ),
        None => item(
            "command_in_service_path",
            "AgentNexus 能找到启动命令",
            "problem",
            "AgentNexus 找不到这个命令".into(),
        ),
    }];
    v.push(match &c.login_shell {
        LoginShell::Skipped => item(
            "command_in_login_shell",
            "终端里能找到启动命令",
            "skipped",
            "自定义命令不做这项检查".into(),
        ),
        LoginShell::Found(p) => item(
            "command_in_login_shell",
            "终端里能找到启动命令",
            "ok",
            format!("终端里找到了：{}", p.display()),
        ),
        LoginShell::NotFound => item(
            "command_in_login_shell",
            "终端里能找到启动命令",
            "problem",
            "终端里也找不到这个命令".into(),
        ),
        LoginShell::Error(e) => item(
            "command_in_login_shell",
            "终端里能找到启动命令",
            "error",
            format!("没能完成检查：{e}"),
        ),
    });
    v.push(match &c.node {
        NodeCheck::Skipped => item(
            "node_version",
            "Node.js 版本",
            "skipped",
            "这个助手不需要 Node.js".into(),
        ),
        NodeCheck::Version { major, text } => {
            let min = c.node_min_major.unwrap_or(0);
            if *major < min {
                item(
                    "node_version",
                    "Node.js 版本",
                    "problem",
                    format!("当前是 {text}，需要 {min} 或更高"),
                )
            } else {
                item(
                    "node_version",
                    "Node.js 版本",
                    "ok",
                    format!("当前是 {text}"),
                )
            }
        }
        NodeCheck::NotFound => item(
            "node_version",
            "Node.js 版本",
            "problem",
            "找不到 node".into(),
        ),
        NodeCheck::Error(e) => item(
            "node_version",
            "Node.js 版本",
            "error",
            format!("没能完成检查：{e}"),
        ),
    });
    if !c.credentials.is_empty() {
        let missing: Vec<&str> = c
            .credentials
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(f, _)| f.as_str())
            .collect();
        v.push(if missing.is_empty() {
            item(
                "credential_file",
                "登录凭证文件",
                "ok",
                "存在（只检查了是否存在，没有读取内容）".into(),
            )
        } else {
            item(
                "credential_file",
                "登录凭证文件",
                "problem",
                format!("不存在：{}", missing.join("、")),
            )
        });
    }
    v
}

/// 错误输出里"找不到 node"的特征（统一小写比较）
const NODE_MISSING: &[&str] = &[
    "env: node",
    "node: no such file",
    "node: not found",
    "node: command not found",
];

fn step(text: &str, command: Option<&str>) -> InstallStep {
    InstallStep {
        platform: "all".into(),
        text: text.into(),
        command: command.map(String::from),
    }
}

/// 规则需要的目录信息（来自 catalog）
#[derive(Debug, Clone, Default)]
pub struct RuleContext {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub install: Vec<InstallStep>,
    pub login: Option<String>,
}

/// 已知问题规则库：按检测结果和体检结果给出结论。检测成功时不给结论
pub fn rules(probe: &ProbeOutcome, c: &Checkup, ctx: &RuleContext) -> Vec<Finding> {
    let mut out = vec![];
    if probe.kind == ProbeKind::Ok {
        return out;
    }
    let detail = probe.detail.clone().unwrap_or_default().to_lowercase();
    let name = &ctx.name;

    // 1. 服务找不到、终端能找到：PATH 不一致（服务方式启动时 PATH 往往不完整）
    if c.in_service_path.is_none() {
        if let LoginShell::Found(p) = &c.login_shell {
            let dir = p
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            out.push(Finding {
                rule: "path_mismatch".into(),
                title: "AgentNexus 找不到命令，但终端里能找到".into(),
                explain: format!(
                    "「{name}」已经装好了，终端里能找到它：{}。但 AgentNexus 启动时的 PATH 里没有这个目录（用桌面应用或服务方式启动时常见）。",
                    p.display()
                ),
                steps: vec![
                    step(
                        &format!("办法一：把 {dir} 加进启动 AgentNexus 的环境变量 PATH，然后重启 AgentNexus"),
                        None,
                    ),
                    step(
                        "办法二：在「助手」页签添加自定义助手，启动命令填下面的完整路径",
                        Some(&p.display().to_string()),
                    ),
                ],
            });
        } else if probe.kind == ProbeKind::NotInstalled
            || matches!(c.login_shell, LoginShell::NotFound | LoginShell::Skipped)
        {
            out.push(Finding {
                rule: "not_installed".into(),
                title: format!("「{name}」还没有安装"),
                explain: format!("AgentNexus 和终端里都找不到启动命令 `{}`。按下面的步骤安装后，再点「重新检测」。", ctx.command),
                steps: ctx.install.clone(),
            });
        }
    }

    // 2. 找不到 node：错误输出里有特征，或体检没找到 node
    let node_missing_in_output = NODE_MISSING.iter().any(|p| detail.contains(p));
    if node_missing_in_output || c.node == NodeCheck::NotFound {
        out.push(Finding {
            rule: "node_missing".into(),
            title: "找不到 Node.js".into(),
            explain: format!(
                "「{name}」需要 Node.js 才能运行，但 AgentNexus 启动它时找不到 node。可能是没装 Node.js，或装了但不在 AgentNexus 的 PATH 里。"
            ),
            steps: vec![
                step("安装 Node.js（建议 22 或更高），官网：https://nodejs.org", None),
                step("装好后在终端确认版本", Some("node --version")),
                step("如果终端里有 node 但 AgentNexus 仍找不到，把 node 所在目录加进启动 AgentNexus 的 PATH 后重启", None),
            ],
        });
    }

    // 3. Node 版本太低
    if let (NodeCheck::Version { major, text }, Some(min)) = (&c.node, c.node_min_major) {
        if *major < min {
            out.push(Finding {
                rule: "node_too_old".into(),
                title: "Node.js 版本太低".into(),
                explain: format!("「{name}」需要 Node.js {min} 或更高，本机是 {text}。"),
                steps: vec![
                    step(
                        &format!("升级 Node.js 到 {min} 或更高，官网：https://nodejs.org"),
                        None,
                    ),
                    step("升级后确认版本", Some("node --version")),
                ],
            });
        }
    }

    // 4. 缺登录凭证文件
    let missing: Vec<&str> = c
        .credentials
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(f, _)| f.as_str())
        .collect();
    if !missing.is_empty() {
        out.push(Finding {
            rule: "credential_missing".into(),
            title: format!("缺少「{name}」的登录凭证"),
            explain: format!("找不到 {}。通常登录一次就会自动生成。", missing.join("、")),
            steps: vec![step(
                ctx.login.as_deref().unwrap_or("按这个助手的说明登录一次"),
                None,
            )],
        });
    }

    // 5. 需要登录
    if probe.kind == ProbeKind::NeedsLogin {
        out.push(Finding {
            rule: "needs_login".into(),
            title: format!("「{name}」需要先登录"),
            explain: "它启动成功了，但要求先登录才能工作。".into(),
            steps: vec![step(
                ctx.login.as_deref().unwrap_or("按这个助手的说明登录"),
                None,
            )],
        });
    }

    // 6. 超时
    if probe.kind == ProbeKind::Timeout {
        let cmdline = [vec![ctx.command.clone()], ctx.args.clone()]
            .concat()
            .join(" ");
        out.push(Finding {
            rule: "timeout".into(),
            title: "没有在规定时间内响应".into(),
            explain: format!(
                "「{name}」启动了，但没有按 ACP 协议回应。常见原因：启动参数不对（没进 ACP 模式），或者第一次启动较慢。"
            ),
            steps: vec![
                step("先再点一次「重新检测」（第一次启动可能要下载或初始化）", None),
                step("仍然超时：确认启动命令是 ACP 模式，当前是", Some(&cmdline)),
            ],
        });
    }
    out
}

// ───────────────────────── 第 3 层：诊断包 ─────────────────────────

/// 组装诊断包需要的信息
pub struct PackageInput<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub command: &'a str,
    pub args: &'a [String],
    /// 环境变量的名字（不带值）
    pub env_names: Vec<String>,
    pub probe: &'a ProbeOutcome,
    pub checks: &'a [CheckItem],
    pub findings: &'a [Finding],
}

/// 诊断包全文（已做本机确定性处理，见 [`scrub_local`]）。
/// 只包含排查需要的信息：助手、启动命令、检测结果、体检、已知原因、系统和版本；环境变量只列名字
pub fn package_text(input: &PackageInput) -> String {
    let mut out = String::new();
    out.push_str("AgentNexus 诊断包（助手接入失败）\n");
    out.push_str(&format!("助手：{}（{}）\n", input.name, input.id));
    let mut cmd = input.command.to_string();
    for a in input.args {
        cmd.push(' ');
        cmd.push_str(a);
    }
    out.push_str(&format!("启动命令：{cmd}\n"));
    out.push_str(&format!(
        "环境变量（只列名字）：{}\n",
        if input.env_names.is_empty() {
            "无".to_string()
        } else {
            input.env_names.join("、")
        }
    ));
    out.push_str(&format!(
        "检测结果：{}\n",
        serde_json::to_value(input.probe.kind)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default()
    ));
    out.push_str(&format!("说明：{}\n", input.probe.summary));
    if let Some(d) = &input.probe.detail {
        out.push_str(&format!("错误详情：{d}\n"));
    }
    out.push_str("本机体检：\n");
    for c in input.checks {
        out.push_str(&format!("- {}：{}（{}）\n", c.name, c.detail, c.status));
    }
    if input.findings.is_empty() {
        out.push_str("已知原因：没有命中已知规则\n");
    } else {
        let titles: Vec<&str> = input.findings.iter().map(|f| f.title.as_str()).collect();
        out.push_str(&format!("已知原因：{}\n", titles.join("；")));
    }
    out.push_str(&format!(
        "系统：{} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str(&format!("AgentNexus 版本：{}\n", env!("CARGO_PKG_VERSION")));
    scrub_local(&out)
}

/// 本机确定性处理（不依赖 DLP 的识别能力）：家目录换成 `~`，本机用户名换成 `<user>`，主机名换成 `<host>`。
/// 3 个字符及以上的名字直接替换；更短的（例如主机名 `vm`）只按整词替换（前后都不是字母、数字、`_`、`-`），
/// 避免把正常文字改坏；1 个字符的不替换
pub fn scrub_local(text: &str) -> String {
    let mut out = text.to_string();
    if let Some(home) = std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned()) {
        let home = home.trim_end_matches('/');
        if home.len() > 1 {
            out = out.replace(home, "~");
        }
    }
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default();
    out = replace_name(&out, &user, "<user>");
    for host in hostnames() {
        out = replace_name(&out, &host, "<host>");
    }
    out
}

/// 按 [`scrub_local`] 的规则替换一个名字
fn replace_name(text: &str, name: &str, with: &str) -> String {
    match name.chars().count() {
        0 | 1 => text.to_string(),
        2 => replace_word(text, name, with),
        _ => text.replace(name, with),
    }
}

/// 只替换整词出现的 `word`（前后都不是字母、数字、`_`、`-`）
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(word) {
        let before = rest[..pos].chars().next_back();
        let after = rest[pos + word.len()..].chars().next();
        let boundary = !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char);
        out.push_str(&rest[..pos]);
        out.push_str(if boundary { with } else { word });
        rest = &rest[pos + word.len()..];
    }
    out.push_str(rest);
    out
}

/// 本机主机名：完整名在前，再加上第一个点之前的短名
fn hostnames() -> Vec<String> {
    let mut names = vec![];
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: gethostname 只往给定长度的缓冲区里写，结尾不一定有 0，下面按 0 或长度截取
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if rc == 0 {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let full = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !full.is_empty() {
                if let Some(short) = full.split('.').next().filter(|s| *s != full) {
                    let short = short.to_string();
                    names.push(full);
                    names.push(short);
                } else {
                    names.push(full);
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {

    #[test]
    fn short_names_are_replaced_only_as_whole_words() {
        assert_eq!(
            replace_word("zhang@vm 出错", "vm", "<host>"),
            "zhang@<host> 出错"
        );
        assert_eq!(
            replace_word("evm vmx vm-1 vm", "vm", "<host>"),
            "evm vmx vm-1 <host>"
        );
        assert_eq!(replace_name("a b", "a", "<user>"), "a b");
        assert_eq!(
            replace_name("home/zhangsan/x", "zhangsan", "<user>"),
            "home/<user>/x"
        );
    }

    use super::*;

    #[test]
    fn node_major_parsing() {
        assert_eq!(parse_node_major("v22.3.0\n"), Some(22));
        assert_eq!(parse_node_major("18.19.0"), Some(18));
        assert_eq!(parse_node_major("garbage"), None);
    }

    #[test]
    fn find_in_path_does_not_use_shell() {
        // 命令名里的特殊字符不会被解释：只是拼成文件路径去查
        assert_eq!(
            find_in_path("/usr/bin", "definitely-not-here;rm -rf /"),
            None
        );
    }
}
