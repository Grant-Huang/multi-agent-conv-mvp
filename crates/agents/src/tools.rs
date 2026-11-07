//! General Agent 可调用的内置工具。
//!
//! 安全边界：所有文件类工具都被限制在配置的 `workspace_dir` 内，
//! 通过路径规范化防止 `../` 越界访问。
//!
//! MCP 适配器见 `mcp` 模块，与内置工具共用此注册表和 General 调用入口。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::capability::{CapabilityCard, Effect};
use an_core::{Artifact, ToolInfo};

use crate::proc_group::{self, GroupChild};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// 工具执行结果
#[derive(Debug)]
pub struct ToolOutput {
    /// 返回给模型看的文本
    pub text: String,
    /// 如果工具产出了文件，作为产物返回
    pub artifacts: Vec<Artifact>,
}

impl ToolOutput {
    fn text(t: impl Into<String>) -> Self {
        Self {
            text: t.into(),
            artifacts: vec![],
        }
    }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数说明（给模型看的简短 JSON 示例）
    fn args_hint(&self) -> &str;
    fn capability(&self) -> CapabilityCard {
        CapabilityCard {
            capability_id: self.name().into(),
            tool_id: self.name().into(),
            description: self.description().into(),
            provider: "builtin".into(),
            effect: if self.name() == "write_file" {
                Effect::WorkspaceWrite
            } else {
                Effect::ReadOnly
            },
            input_schema: args_schema(),
            output_schema: None,
            input_types: vec!["application/json".into()],
            output_types: vec!["text/plain".into()],
        }
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput>;
}

/// 工具注册表：Router 选择工具、Policy 校验工具名都依赖它
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn capabilities(&self) -> Vec<CapabilityCard> {
        self.tools.values().map(|tool| tool.capability()).collect()
    }

    pub fn extend(&mut self, other: Self) -> Result<()> {
        if other.tools.keys().any(|name| self.tools.contains_key(name)) {
            bail!("E_MCP_CONFIG: duplicate tool ID");
        }
        self.tools.extend(other.tools);
        Ok(())
    }

    pub async fn call(&self, name: &str, args: &Value) -> Result<ToolOutput> {
        self.get(name)
            .ok_or_else(|| anyhow!("E_TOOL_UNKNOWN: {name}"))?
            .call(args)
            .await
    }
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn infos(&self) -> Vec<ToolInfo> {
        self.tools
            .values()
            .map(|t| ToolInfo {
                name: t.name().into(),
                description: t.description().into(),
            })
            .collect()
    }

    /// 给模型看的工具清单
    pub fn prompt_listing(&self) -> String {
        self.tools
            .values()
            .map(|t| {
                format!(
                    "- {}：{}。参数示例：{}",
                    t.name(),
                    t.description(),
                    t.args_hint()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 创建默认工具集
    pub fn with_builtin(workspace_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(workspace_dir)
            .with_context(|| format!("无法创建工作目录 {}", workspace_dir.display()))?;
        let root = workspace_dir.canonicalize()?;
        let mut r = ToolRegistry::default();
        r.register(Arc::new(CurrentTime));
        r.register(Arc::new(Calculator));
        r.register(Arc::new(ListFiles { root: root.clone() }));
        r.register(Arc::new(ReadFile { root: root.clone() }));
        r.register(Arc::new(WriteFile { root }));
        Ok(r)
    }

    /// 命令存在时注册 `web_search` 和 `web_fetch`。不存在或命令为空则什么都不做。
    pub fn register_websearch(&mut self, command: &str, timeout: Duration) -> bool {
        if command.is_empty() || !command_exists(command) {
            return false;
        }
        let cmd = WebCmd {
            command: command.to_string(),
            timeout,
        };
        self.register(Arc::new(WebSearch { cmd: cmd.clone() }));
        self.register(Arc::new(WebFetch { cmd }));
        true
    }
}

fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if command.contains('/') || command.contains('\\') {
        return path.is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(command).is_file()))
}

#[derive(Clone)]
struct WebCmd {
    command: String,
    timeout: Duration,
}

/// 子进程的输出
struct Collected {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// 运行搜索命令并收集输出。子进程放在独立进程组里：
/// 超时时整个组先 SIGTERM 再 SIGKILL；正常退出后最多再读 DRAIN_AFTER_EXIT 的剩余输出，
/// 然后清理组内残留进程（孙进程继承了管道时，不能等管道关闭，否则会一直等到超时）。
async fn run_collect(cmd: &WebCmd, args: &[&str]) -> Result<Collected> {
    use tokio::io::AsyncReadExt;

    let mut command = tokio::process::Command::new(&cmd.command);
    command
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    proc_group::configure(&mut command);
    let mut child =
        GroupChild::spawn(&mut command, "").with_context(|| format!("无法启动 {}", cmd.command))?;
    let mut out_pipe = child
        .child_mut()
        .stdout
        .take()
        .context("没有拿到标准输出")?;
    let mut err_pipe = child
        .child_mut()
        .stderr
        .take()
        .context("没有拿到标准错误")?;

    let collect = async {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let (mut out_buf, mut err_buf) = ([0u8; 8192], [0u8; 8192]);
        let (mut out_done, mut err_done) = (false, false);
        let mut exit = None;
        let mut drain_until = tokio::time::Instant::now();
        while !(out_done && err_done) {
            tokio::select! {
                status = child.wait(), if exit.is_none() => {
                    exit = Some(status);
                    drain_until = tokio::time::Instant::now() + proc_group::DRAIN_AFTER_EXIT;
                },
                _ = tokio::time::sleep_until(drain_until), if exit.is_some() => break,
                n = out_pipe.read(&mut out_buf), if !out_done => match n {
                    Ok(0) | Err(_) => out_done = true,
                    Ok(n) => stdout.extend_from_slice(&out_buf[..n]),
                },
                n = err_pipe.read(&mut err_buf), if !err_done => match n {
                    Ok(0) | Err(_) => err_done = true,
                    Ok(n) => stderr.extend_from_slice(&err_buf[..n]),
                },
            }
        }
        match exit {
            Some(status) => status,
            None => child.wait().await,
        }
        .map(|_| Collected { stdout, stderr })
    };
    match tokio::time::timeout(cmd.timeout, collect).await {
        Ok(Ok(out)) => {
            child.kill_leftovers();
            Ok(out)
        }
        Ok(Err(e)) => {
            child.kill_leftovers();
            Err(e).with_context(|| format!("{} 执行失败", cmd.command))
        }
        Err(_) => {
            child.terminate("timeout").await;
            bail!("{} 超时（{} 秒）", cmd.command, cmd.timeout.as_secs())
        }
    }
}

async fn run_websearch(cmd: &WebCmd, args: &[&str]) -> Result<String> {
    let out = run_collect(cmd, args).await?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{} 没有输出：{}", cmd.command, err.trim());
    }
    let value: Value = serde_json::from_str(&stdout).context("搜索命令没有返回 JSON")?;
    if value.get("status").and_then(|s| s.as_str()) != Some("success") {
        let message = value
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("搜索失败");
        bail!("{message}");
    }
    // 原样返回：截断会破坏 JSON，研究报告解析链接会失败；
    // 逐步循环若嫌长，由 General 写历史时再 clip_middle。
    Ok(stdout)
}

/// 读取字符串参数
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("缺少字符串参数 `{key}`"))
}

/// 把用户给的相对路径解析到工作目录内；越界则报错。
/// 不依赖文件是否存在（写文件时目标还不存在）。
fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || !out.starts_with(root) {
                    bail!("路径越界：{rel}");
                }
            }
            // 绝对路径、Windows 盘符一律拒绝
            Component::RootDir | Component::Prefix(_) => bail!("只允许工作目录内的相对路径：{rel}"),
        }
    }
    if !out.starts_with(root) {
        bail!("路径越界：{rel}");
    }
    Ok(out)
}

// ───────────── current_time ─────────────

struct CurrentTime;

#[async_trait::async_trait]
impl Tool for CurrentTime {
    fn name(&self) -> &str {
        "current_time"
    }
    fn description(&self) -> &str {
        "获取当前本地日期和时间"
    }
    fn args_hint(&self) -> &str {
        "{}"
    }
    async fn call(&self, _args: &Value) -> Result<ToolOutput> {
        Ok(ToolOutput::text(
            chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S %:z（%A）")
                .to_string(),
        ))
    }
}

// ───────────── calculator ─────────────

/// 四则运算计算器（小模型算数不可靠，交给工具）
struct Calculator;

#[async_trait::async_trait]
impl Tool for Calculator {
    fn name(&self) -> &str {
        "calculator"
    }
    fn description(&self) -> &str {
        "计算数学表达式，支持 + - * / % ^ 和括号"
    }
    fn args_hint(&self) -> &str {
        r#"{"expression": "(12.5 + 3) * 4"}"#
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let expr = arg_str(args, "expression")?;
        let v = calc::eval(expr)?;
        Ok(ToolOutput::text(format!("{expr} = {v}")))
    }
}

/// 一个很小的递归下降表达式解析器
mod calc {
    use anyhow::{bail, Result};

    pub fn eval(s: &str) -> Result<f64> {
        let tokens: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        let mut p = Parser { t: tokens, i: 0 };
        let v = p.expr()?;
        if p.i != p.t.len() {
            bail!("表达式在第 {} 个字符处无法解析", p.i + 1);
        }
        if !v.is_finite() {
            bail!("结果不是有限数（可能除以了 0）");
        }
        Ok(v)
    }

    struct Parser {
        t: Vec<char>,
        i: usize,
    }

    impl Parser {
        fn peek(&self) -> Option<char> {
            self.t.get(self.i).copied()
        }
        // expr := term (('+'|'-') term)*
        fn expr(&mut self) -> Result<f64> {
            let mut v = self.term()?;
            while let Some(c @ ('+' | '-')) = self.peek() {
                self.i += 1;
                let r = self.term()?;
                v = if c == '+' { v + r } else { v - r };
            }
            Ok(v)
        }
        // term := power (('*'|'/'|'%') power)*
        fn term(&mut self) -> Result<f64> {
            let mut v = self.power()?;
            while let Some(c @ ('*' | '/' | '%')) = self.peek() {
                self.i += 1;
                let r = self.power()?;
                v = match c {
                    '*' => v * r,
                    '/' => v / r,
                    _ => v % r,
                };
            }
            Ok(v)
        }
        // power := unary ('^' power)?   （右结合）
        fn power(&mut self) -> Result<f64> {
            let base = self.unary()?;
            if self.peek() == Some('^') {
                self.i += 1;
                let exp = self.power()?;
                return Ok(base.powf(exp));
            }
            Ok(base)
        }
        // unary := '-' unary | atom
        fn unary(&mut self) -> Result<f64> {
            if self.peek() == Some('-') {
                self.i += 1;
                return Ok(-self.unary()?);
            }
            self.atom()
        }
        // atom := number | '(' expr ')'
        fn atom(&mut self) -> Result<f64> {
            match self.peek() {
                Some('(') => {
                    self.i += 1;
                    let v = self.expr()?;
                    if self.peek() != Some(')') {
                        bail!("缺少右括号");
                    }
                    self.i += 1;
                    Ok(v)
                }
                Some(c) if c.is_ascii_digit() || c == '.' => {
                    let start = self.i;
                    while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
                        self.i += 1;
                    }
                    let s: String = self.t[start..self.i].iter().collect();
                    Ok(s.parse()?)
                }
                Some(c) => bail!("无法识别的字符 `{c}`"),
                None => bail!("表达式意外结束"),
            }
        }
    }
}

// ───────────── 文件工具 ─────────────

struct ListFiles {
    root: PathBuf,
}

#[async_trait::async_trait]
impl Tool for ListFiles {
    fn name(&self) -> &str {
        "list_files"
    }
    fn description(&self) -> &str {
        "列出工作目录下某个子目录的文件（path 为空表示根目录）"
    }
    fn args_hint(&self) -> &str {
        r#"{"path": ""}"#
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let rel = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let dir = resolve_in_root(&self.root, rel)?;
        let mut entries = vec![];
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .with_context(|| format!("无法读取目录 {rel}"))?;
        while let Some(e) = rd.next_entry().await? {
            let is_dir = e.file_type().await?.is_dir();
            entries.push(format!(
                "{}{}",
                e.file_name().to_string_lossy(),
                if is_dir { "/" } else { "" }
            ));
        }
        entries.sort();
        Ok(ToolOutput::text(if entries.is_empty() {
            "（空目录）".into()
        } else {
            entries.join("\n")
        }))
    }
}

struct ReadFile {
    root: PathBuf,
}

/// 读文件时最多返回的字符数，避免撑爆小模型的上下文
const READ_LIMIT: usize = 8000;

#[async_trait::async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "读取工作目录中的文本文件"
    }
    fn args_hint(&self) -> &str {
        r#"{"path": "notes/a.md"}"#
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let rel = arg_str(args, "path")?;
        let p = resolve_in_root(&self.root, rel)?;
        let text = tokio::fs::read_to_string(&p)
            .await
            .with_context(|| format!("无法读取文件 {rel}"))?;
        Ok(ToolOutput::text(an_core::truncate_chars(&text, READ_LIMIT)))
    }
}

/// 续写内容里有一段不短于 DUPLICATE_RUN 字的文字已经在文件里出现过
const DUPLICATE_RUN: usize = 200;

fn repeats_existing_text(existing: &str, addition: &str) -> bool {
    let chars: Vec<char> = addition.chars().collect();
    if chars.len() < DUPLICATE_RUN {
        return false;
    }
    let step = DUPLICATE_RUN / 4;
    let mut start = 0;
    loop {
        let end = (start + DUPLICATE_RUN).min(chars.len());
        let window: String = chars[end - DUPLICATE_RUN..end].iter().collect();
        if existing.contains(&window) {
            return true;
        }
        if end == chars.len() {
            return false;
        }
        start += step;
    }
}

struct WriteFile {
    root: PathBuf,
}

#[async_trait::async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "在工作目录中写入文本文件（如 Markdown 报告）。mode=overwrite 覆盖，mode=append 把 content 接到文件末尾。续写必须用 append，不要重写开头"
    }
    fn args_hint(&self) -> &str {
        r##"{"path": "reports/report.md", "content": "# 标题\n正文", "mode": "overwrite"}"##
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let rel = arg_str(args, "path")?;
        let content = arg_str(args, "content")?;
        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("overwrite");
        if mode != "overwrite" && mode != "append" {
            bail!("mode 只能是 overwrite 或 append");
        }
        let p = resolve_in_root(&self.root, rel)?;
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let stored = if mode == "append" && p.exists() {
            let prev = tokio::fs::read_to_string(&p)
                .await
                .with_context(|| format!("无法读取已有文件 {rel}"))?;
            if repeats_existing_text(&prev, content) {
                bail!("{rel} 里已经写过这段内容，未追加。content 只放尚未写出的新章节");
            }
            format!("{prev}{content}")
        } else {
            content.to_string()
        };
        tokio::fs::write(&p, &stored)
            .await
            .with_context(|| format!("无法写入文件 {rel}"))?;
        let n = stored.chars().count();
        let tail: String = stored.chars().skip(n.saturating_sub(80)).collect();
        Ok(ToolOutput {
            text: format!(
                "已写入 {rel}（{n} 字符，{mode}）。末尾：{tail}\n全部写完：下一步 action=final。还没写完：下一步 write_file 且 mode=append，content 只放新增章节，最后一次写入带上 answer。"
            ),
            artifacts: vec![Artifact {
                kind: "file".into(),
                name: rel.into(),
                content: stored,
            }],
        })
    }
}

struct WebSearch {
    cmd: WebCmd,
}

#[async_trait::async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "联网搜索并读取前几条网页正文。默认用 Exa。需要最新资料、外部事实或用户要求搜索时使用"
    }
    fn args_hint(&self) -> &str {
        r#"{"query": "2026-11-05 AI agent 架构", "max_results": 3}"#
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let query = arg_str(args, "query")?;
        let n = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(3)
            .clamp(1, 8);
        let n = n.to_string();
        // fetch=false：只返回标题/链接/摘要，研究报告用这个就够，不必再抓全文
        let fetch = args.get("fetch").and_then(|v| v.as_bool()).unwrap_or(true);
        let mut argv = if fetch {
            vec![
                "search-fetch".to_string(),
                query.to_string(),
                "-n".into(),
                n,
                "--max-chars".into(),
                "3000".into(),
            ]
        } else {
            vec!["search".to_string(), query.to_string(), "-n".into(), n]
        };
        if let Some(provider) = args
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            argv.push("--provider".into());
            argv.push(provider.to_string());
        }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let text = run_websearch(&self.cmd, &refs).await?;
        Ok(ToolOutput::text(text))
    }
}

struct WebFetch {
    cmd: WebCmd,
}

#[async_trait::async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "抓取一个 http(s) 网页的正文"
    }
    fn args_hint(&self) -> &str {
        r#"{"url": "https://example.com/page"}"#
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let url = arg_str(args, "url")?;
        let text = run_websearch(&self.cmd, &["fetch", url, "--max-chars", "8000"]).await?;
        Ok(ToolOutput::text(text))
    }
}

/// 工具参数的 JSON Schema：任意对象（具体参数由 args_hint 引导）
pub fn args_schema() -> Value {
    json!({ "type": "object" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_works() {
        assert_eq!(calc::eval("(12.5 + 3) * 4").unwrap(), 62.0);
        assert_eq!(calc::eval("2^3^2").unwrap(), 512.0);
        assert_eq!(calc::eval("-3 + 10 % 4").unwrap(), -1.0);
        assert!(calc::eval("1/0").is_err());
        assert!(calc::eval("2 +").is_err());
    }

    fn write_script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("websearch");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&path, perm).unwrap();
        }
        path
    }

    #[tokio::test]
    async fn websearch_missing_command_is_not_registered() {
        let dir = std::env::temp_dir().join(format!("an-websearch-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
        assert!(!tools.register_websearch("/no/such/websearch", Duration::from_secs(1)));
        assert!(!tools.names().iter().any(|n| n == "web_search"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn websearch_parses_success_json() {
        let dir = std::env::temp_dir().join(format!("an-websearch-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_script(
            &dir,
            "#!/bin/sh\nprintf '%s' '{\"status\":\"success\",\"data\":{\"pages\":[{\"url\":\"https://example.com\",\"content\":\"正文\"}]},\"message\":\"\"}'\n",
        );
        let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
        assert!(tools.register_websearch(script.to_str().unwrap(), Duration::from_secs(2)));
        let out = tools
            .get("web_search")
            .unwrap()
            .call(&json!({"query": "q"}))
            .await
            .unwrap();
        assert!(out.text.contains("正文"));
        assert!(out.text.contains("https://example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn websearch_passes_provider() {
        let dir =
            std::env::temp_dir().join(format!("an-websearch-provider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_script(
            &dir,
            "#!/bin/sh\ncase \"$*\" in *'--provider parallel'*) printf '%s' '{\"status\":\"success\",\"data\":{\"provider\":\"parallel\"},\"message\":\"\"}' ;; *) printf '%s' '{\"status\":\"error\",\"data\":{},\"message\":\"missing\"}' ;; esac\n",
        );
        let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
        assert!(tools.register_websearch(script.to_str().unwrap(), Duration::from_secs(2)));
        let out = tools
            .get("web_search")
            .unwrap()
            .call(&json!({"query": "q", "provider": "parallel"}))
            .await
            .unwrap();
        assert!(out.text.contains("parallel"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn websearch_timeout() {
        let dir = std::env::temp_dir().join(format!("an-websearch-slow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_script(&dir, "#!/bin/sh\nsleep 5\n");
        let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
        assert!(tools.register_websearch(script.to_str().unwrap(), Duration::from_millis(200)));
        let message = match tools
            .get("web_fetch")
            .unwrap()
            .call(&json!({"url": "https://example.com"}))
            .await
        {
            Err(e) => e.to_string(),
            Ok(_) => panic!("应当超时"),
        };
        assert!(message.contains("超时"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 搜索命令超时：整个进程组被结束，脚本起的孙进程不残留（仅 Unix）
    #[cfg(unix)]
    #[tokio::test]
    async fn websearch_timeout_kills_group() {
        use crate::test_proc::*;
        let dir = std::env::temp_dir().join(an_core::new_id("an-websearch-group"));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("gc.pid");
        let script = write_script(
            &dir,
            &format!(
                "#!/bin/sh\nsleep 300 &\necho $! > {}\nwait\n",
                pid_file.display()
            ),
        );
        let cmd = WebCmd {
            command: script.to_string_lossy().into_owned(),
            // 并行跑全量测试时机器忙，500ms 可能在脚本写出 pid 之前就超时
            timeout: Duration::from_millis(2000),
        };
        let err = run_websearch(&cmd, &["search", "q"])
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("超时"), "{err}");
        let pid = read_pid(&pid_file, 5);
        let dead = wait_dead(pid, 5);
        force_kill(pid);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(dead, "超时后孙进程 {pid} 还在运行");
    }

    /// 搜索命令已经输出并退出、孙进程还占着管道：立即返回结果，不等到超时
    #[cfg(unix)]
    #[tokio::test]
    async fn websearch_exited_command_does_not_hang_on_inherited_pipe() {
        use crate::test_proc::*;
        let dir = std::env::temp_dir().join(an_core::new_id("an-websearch-pipe"));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("gc.pid");
        let script = write_script(
            &dir,
            &format!("#!/bin/sh\nsleep 300 &\necho $! > {}\nprintf '%s' '{{\"status\":\"success\",\"data\":{{}},\"message\":\"\"}}'\n", pid_file.display()),
        );
        let cmd = WebCmd {
            command: script.to_string_lossy().into_owned(),
            timeout: Duration::from_secs(10),
        };
        let started = std::time::Instant::now();
        let out = run_websearch(&cmd, &["search", "q"]).await;
        let elapsed = started.elapsed();
        let pid = read_pid(&pid_file, 2);
        let dead = wait_dead(pid, 5);
        force_kill(pid);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.unwrap().contains("success"));
        assert!(
            elapsed < Duration::from_secs(5),
            "命令退出后应尽快返回，实际用了 {elapsed:?}"
        );
        assert!(dead, "残留的孙进程 {pid} 应被清理");
    }

    #[tokio::test]
    async fn write_file_append_keeps_the_existing_head() {
        let dir = std::env::temp_dir().join(format!("an-write-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tools = ToolRegistry::with_builtin(&dir).unwrap();
        let write = tools.get("write_file").unwrap();
        write
            .call(&json!({"path":"reports/r.md","content":"# 标题\n开头","mode":"overwrite"}))
            .await
            .unwrap();
        let out = write
            .call(&json!({"path":"reports/r.md","content":"\n后续","mode":"append"}))
            .await
            .unwrap();
        let disk = std::fs::read_to_string(dir.join("reports/r.md")).unwrap();
        assert_eq!(disk, "# 标题\n开头\n后续");
        assert!(out.text.contains("append"));
        assert!(out.text.contains("后续"));
        assert!(!out.artifacts[0].content.contains("正文已省略"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn write_file_append_rejects_text_already_in_the_file() {
        let dir = std::env::temp_dir().join(format!("an-write-dup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tools = ToolRegistry::with_builtin(&dir).unwrap();
        let write = tools.get("write_file").unwrap();
        let section = format!(
            "## 五、情景分析\n{}",
            "红蓝政府与分裂政府的影响。".repeat(30)
        );
        write
            .call(&json!({"path":"r.md","content":format!("# 报告\n{section}"),"mode":"overwrite"}))
            .await
            .unwrap();
        let err = write
            .call(&json!({"path":"r.md","content":format!("\n{section}"),"mode":"append"}))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("已经写过"), "{err:#}");
        let disk = std::fs::read_to_string(dir.join("r.md")).unwrap();
        assert_eq!(disk.matches("## 五、情景分析").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_guard() {
        let root = Path::new("/tmp/ws");
        assert_eq!(
            resolve_in_root(root, "a/b.md").unwrap(),
            Path::new("/tmp/ws/a/b.md")
        );
        assert_eq!(
            resolve_in_root(root, "a/../b.md").unwrap(),
            Path::new("/tmp/ws/b.md")
        );
        assert!(resolve_in_root(root, "../etc/passwd").is_err());
        assert!(resolve_in_root(root, "/etc/passwd").is_err());
    }
}
