//! General Agent 可调用的内置工具。
//!
//! 安全边界：所有文件类工具都被限制在配置的 `workspace_dir` 内，
//! 通过路径规范化防止 `../` 越界访问。
//!
//! 以后接 MCP：实现同一个 `Tool` trait，内部用 rmcp 客户端转发调用即可，
//! General Agent 和 Router 都不需要改。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use an_core::{Artifact, ToolInfo};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// 工具执行结果
pub struct ToolOutput {
    /// 返回给模型看的文本
    pub text: String,
    /// 如果工具产出了文件，作为产物返回
    pub artifact: Option<Artifact>,
}

impl ToolOutput {
    fn text(t: impl Into<String>) -> Self {
        Self { text: t.into(), artifact: None }
    }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// 参数说明（给模型看的简短 JSON 示例）
    fn args_hint(&self) -> &str;
    async fn call(&self, args: &Value) -> Result<ToolOutput>;
}

/// 工具注册表：Router 选择工具、Policy 校验工具名都依赖它
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
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
            .map(|t| ToolInfo { name: t.name().into(), description: t.description().into() })
            .collect()
    }

    /// 给模型看的工具清单
    pub fn prompt_listing(&self) -> String {
        self.tools
            .values()
            .map(|t| format!("- {}：{}。参数示例：{}", t.name(), t.description(), t.args_hint()))
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
        let cmd = WebCmd { command: command.to_string(), timeout };
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
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(command).is_file()))
}

#[derive(Clone)]
struct WebCmd {
    command: String,
    timeout: Duration,
}

async fn run_websearch(cmd: &WebCmd, args: &[&str]) -> Result<String> {
    let child = tokio::process::Command::new(&cmd.command)
        .args(args)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("无法启动 {}", cmd.command))?;
    let out = match tokio::time::timeout(cmd.timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(e).with_context(|| format!("{} 执行失败", cmd.command)),
        Err(_) => bail!("{} 超时（{} 秒）", cmd.command, cmd.timeout.as_secs()),
    };
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{} 没有输出：{}", cmd.command, err.trim());
    }
    let value: Value = serde_json::from_str(&stdout).context("搜索命令没有返回 JSON")?;
    if value.get("status").and_then(|s| s.as_str()) != Some("success") {
        let message = value.get("message").and_then(|m| m.as_str()).unwrap_or("搜索失败");
        bail!("{message}");
    }
    Ok(an_core::truncate_chars(&stdout, READ_LIMIT))
}

/// 读取字符串参数
fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key).and_then(|v| v.as_str()).ok_or_else(|| anyhow!("缺少字符串参数 `{key}`"))
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
        Ok(ToolOutput::text(chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z（%A）").to_string()))
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
        let mut rd = tokio::fs::read_dir(&dir).await.with_context(|| format!("无法读取目录 {rel}"))?;
        while let Some(e) = rd.next_entry().await? {
            let is_dir = e.file_type().await?.is_dir();
            entries.push(format!("{}{}", e.file_name().to_string_lossy(), if is_dir { "/" } else { "" }));
        }
        entries.sort();
        Ok(ToolOutput::text(if entries.is_empty() { "（空目录）".into() } else { entries.join("\n") }))
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
        let text = tokio::fs::read_to_string(&p).await.with_context(|| format!("无法读取文件 {rel}"))?;
        Ok(ToolOutput::text(an_core::truncate_chars(&text, READ_LIMIT)))
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
        "在工作目录中写入文本文件（如 Markdown 报告），已存在则覆盖"
    }
    fn args_hint(&self) -> &str {
        r##"{"path": "reports/report.md", "content": "# 标题\n正文"}"##
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        let rel = arg_str(args, "path")?;
        let content = arg_str(args, "content")?;
        let p = resolve_in_root(&self.root, rel)?;
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&p, content).await.with_context(|| format!("无法写入文件 {rel}"))?;
        Ok(ToolOutput {
            text: format!("已写入 {rel}（{} 字符）", content.chars().count()),
            artifact: Some(Artifact { kind: "file".into(), name: rel.into(), content: content.into() }),
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
        let n = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(3).clamp(1, 8);
        let n = n.to_string();
        let mut argv = vec!["search-fetch".to_string(), query.to_string(), "-n".into(), n, "--max-chars".into(), "3000".into()];
        if let Some(provider) = args.get("provider").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
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
        let out = tools.get("web_search").unwrap().call(&json!({"query": "q"})).await.unwrap();
        assert!(out.text.contains("正文"));
        assert!(out.text.contains("https://example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn websearch_passes_provider() {
        let dir = std::env::temp_dir().join(format!("an-websearch-provider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_script(
            &dir,
            "#!/bin/sh\ncase \"$*\" in *'--provider parallel'*) printf '%s' '{\"status\":\"success\",\"data\":{\"provider\":\"parallel\"},\"message\":\"\"}' ;; *) printf '%s' '{\"status\":\"error\",\"data\":{},\"message\":\"missing\"}' ;; esac\n",
        );
        let mut tools = ToolRegistry::with_builtin(&dir).unwrap();
        assert!(tools.register_websearch(script.to_str().unwrap(), Duration::from_secs(2)));
        let out = tools.get("web_search").unwrap().call(&json!({"query": "q", "provider": "parallel"})).await.unwrap();
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
        let message = match tools.get("web_fetch").unwrap().call(&json!({"url": "https://example.com"})).await {
            Err(e) => e.to_string(),
            Ok(_) => panic!("应当超时"),
        };
        assert!(message.contains("超时"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_guard() {
        let root = Path::new("/tmp/ws");
        assert_eq!(resolve_in_root(root, "a/b.md").unwrap(), Path::new("/tmp/ws/a/b.md"));
        assert_eq!(resolve_in_root(root, "a/../b.md").unwrap(), Path::new("/tmp/ws/b.md"));
        assert!(resolve_in_root(root, "../etc/passwd").is_err());
        assert!(resolve_in_root(root, "/etc/passwd").is_err());
    }
}
