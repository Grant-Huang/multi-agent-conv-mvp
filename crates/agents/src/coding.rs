//! Coding Agent：外部 Harness 适配器，以子进程方式调用 Codex CLI（非交互模式）。
//!
//! 设计要点：
//! - 命令和参数完全由配置决定，参数里的占位符会被替换：
//!   `{prompt}` 任务描述、`{workdir}` 工作目录、`{last_message_file}` 最后一条回复的输出文件；
//!   不同版本 Codex 的参数可能不同，请用 `codex exec --help` 核对后修改配置；
//! - 子进程设置 `kill_on_drop`：编排层取消任务（abort）时子进程会被一起杀掉；
//! - 执行后在工作目录跑 `git diff`，把改动作为产物和证据交给 Reviewer。
//!   "Codex 说改好了"不等于"改对了"，验收要看证据。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use an_core::{truncate_chars, AgentAdapter, AgentCard, Artifact, EventSink, Evidence, Role, TaskResult, TaskSpec, WorkerStatus};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Coding Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingConfig {
    /// 可执行文件，默认 `codex`
    #[serde(default = "default_command")]
    pub command: String,
    /// 参数模板，支持 `{prompt}` `{workdir}` `{last_message_file}` 占位符
    #[serde(default = "default_args")]
    pub args: Vec<String>,
    /// 代码工作目录（建议指向一个独立的 git 仓库或 worktree）
    #[serde(default = "default_workdir")]
    pub workdir: PathBuf,
    /// 超时时间（秒）
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_command() -> String {
    "codex".into()
}
fn default_args() -> Vec<String> {
    [
        "exec",
        "--skip-git-repo-check",
        "--sandbox",
        "workspace-write",
        "--cd",
        "{workdir}",
        "--output-last-message",
        "{last_message_file}",
        "{prompt}",
    ]
    .map(String::from)
    .to_vec()
}
fn default_workdir() -> PathBuf {
    PathBuf::from("workspace/code")
}
fn default_timeout() -> u64 {
    900
}

impl Default for CodingConfig {
    fn default() -> Self {
        Self { command: default_command(), args: default_args(), workdir: default_workdir(), timeout_secs: default_timeout() }
    }
}

pub struct CodingAgent {
    cfg: CodingConfig,
    card: AgentCard,
}

/// 保留的输出行数（用于摘要和证据）
const TAIL_LINES: usize = 40;

/// 展开路径开头的 `~`（子进程不会经过 shell，`~` 不会被自动展开）
pub fn expand_tilde(s: &str) -> String {
    match (s.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ if s == "~" => std::env::var("HOME").unwrap_or_else(|_| s.into()),
        _ => s.into(),
    }
}

impl CodingAgent {
    /// 创建时检查命令是否可用；不可用时 Agent 仍然注册，但标记为 disabled，Router 不会选它
    pub fn new(mut cfg: CodingConfig) -> Self {
        cfg.command = expand_tilde(&cfg.command);
        cfg.workdir = PathBuf::from(expand_tilde(&cfg.workdir.to_string_lossy()));
        let probe = std::process::Command::new(&cfg.command)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let (enabled, disabled_reason) = match probe {
            Ok(_) => (true, None),
            Err(e) => (false, Some(format!("找不到命令 `{}`：{e}", cfg.command))),
        };
        let card = AgentCard {
            role: Role::Coding,
            name: "Coding Agent (Codex CLI)".into(),
            description: "工程执行：在代码仓库中阅读、修改、调试代码，运行测试。适合明确的代码开发/修复任务".into(),
            tools: vec![],
            enabled,
            disabled_reason,
        };
        Self { cfg, card }
    }

    /// 替换参数模板中的占位符
    fn render_args(&self, prompt: &str, workdir: &str, last_msg: &str) -> Vec<String> {
        self.cfg
            .args
            .iter()
            .map(|a| a.replace("{prompt}", prompt).replace("{workdir}", workdir).replace("{last_message_file}", last_msg))
            .collect()
    }

    /// 把一行输出整理成适合展示的进度文本。
    /// 如果是 JSON（例如 codex 的 --json 模式），尽量提取类型和文本。
    fn describe_line(line: &str) -> String {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let ty = v["type"].as_str().or_else(|| v["msg"]["type"].as_str()).unwrap_or("event");
            let text = v["text"].as_str().or_else(|| v["msg"]["message"].as_str()).unwrap_or("");
            return truncate_chars(&format!("[{ty}] {text}"), 300);
        }
        truncate_chars(line, 300)
    }
}

/// 在工作目录执行 git 命令，返回 stdout（失败返回 None）
async fn git(workdir: &PathBuf, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(workdir).args(args).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[async_trait::async_trait]
impl AgentAdapter for CodingAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        if !self.card.enabled {
            return Ok(TaskResult::failed(self.card.disabled_reason.clone().unwrap_or_default()));
        }

        // 1. 准备工作目录和输出文件
        tokio::fs::create_dir_all(&self.cfg.workdir).await.context("创建代码工作目录失败")?;
        let workdir = self.cfg.workdir.canonicalize()?;
        let last_msg_file = std::env::temp_dir().join(format!("an-codex-{}.txt", task.task_id));
        let _ = tokio::fs::remove_file(&last_msg_file).await;

        let prompt = if task.context.is_empty() {
            task.goal.clone()
        } else {
            format!("{}\n\n## 背景信息\n{}", task.goal, task.context)
        };
        let args = self.render_args(&prompt, &workdir.to_string_lossy(), &last_msg_file.to_string_lossy());
        events.progress(format!("启动 {}（工作目录 {}）", self.cfg.command, workdir.display()));

        // 2. 启动子进程
        let mut child = Command::new(&self.cfg.command)
            .args(&args)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true) // 任务被取消时自动杀掉子进程
            .spawn()
            .with_context(|| format!("无法启动 {}", self.cfg.command))?;

        let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
        let mut stderr = BufReader::new(child.stderr.take().expect("stderr piped")).lines();
        let mut out_tail: VecDeque<String> = VecDeque::new();
        let mut err_tail: VecDeque<String> = VecDeque::new();
        let push = |q: &mut VecDeque<String>, l: String| {
            if q.len() == TAIL_LINES {
                q.pop_front();
            }
            q.push_back(l);
        };

        // 3. 一边读输出（推送进度），一边等待进程结束，整体受超时控制
        let run = async {
            let (mut out_done, mut err_done) = (false, false);
            while !(out_done && err_done) {
                tokio::select! {
                    line = stdout.next_line(), if !out_done => match line {
                        Ok(Some(l)) => { events.progress(Self::describe_line(&l)); push(&mut out_tail, l); }
                        _ => out_done = true,
                    },
                    line = stderr.next_line(), if !err_done => match line {
                        Ok(Some(l)) => push(&mut err_tail, l),
                        _ => err_done = true,
                    },
                }
            }
            child.wait().await
        };
        let exit = match tokio::time::timeout(Duration::from_secs(self.cfg.timeout_secs), run).await {
            Ok(r) => r.context("等待子进程失败")?,
            // 超时：返回后 child 被 drop，kill_on_drop 会杀掉进程
            Err(_) => return Ok(TaskResult::failed(format!("Coding Agent 超时（{} 秒）", self.cfg.timeout_secs))),
        };

        // 4. 收集结果与证据
        let mut evidence = vec![Evidence { kind: "exit_code".into(), content: format!("{exit}") }];
        let stdout_tail = out_tail.iter().cloned().collect::<Vec<_>>().join("\n");
        evidence.push(Evidence { kind: "stdout_tail".into(), content: truncate_chars(&stdout_tail, 3000) });
        if !exit.success() && !err_tail.is_empty() {
            let s = err_tail.iter().cloned().collect::<Vec<_>>().join("\n");
            evidence.push(Evidence { kind: "stderr_tail".into(), content: truncate_chars(&s, 3000) });
        }

        let mut artifacts = vec![];
        match git(&workdir, &["rev-parse", "--is-inside-work-tree"]).await {
            Some(_) => {
                // 包含未跟踪文件：先 add -N（intent-to-add），让新文件也出现在 diff 里
                let _ = git(&workdir, &["add", "-N", "."]).await;
                if let Some(stat) = git(&workdir, &["diff", "--stat"]).await {
                    evidence.push(Evidence { kind: "git_diff_stat".into(), content: if stat.trim().is_empty() { "无改动".into() } else { stat } });
                }
                if let Some(diff) = git(&workdir, &["diff"]).await.filter(|d| !d.trim().is_empty()) {
                    artifacts.push(Artifact { kind: "diff".into(), name: "git diff".into(), content: truncate_chars(&diff, 20000) });
                }
            }
            None => evidence.push(Evidence { kind: "git_diff_stat".into(), content: "工作目录不是 git 仓库，无法收集代码改动".into() }),
        }

        // 摘要优先使用 Codex 写出的最后一条回复，否则用输出末尾几行
        let summary = match tokio::fs::read_to_string(&last_msg_file).await {
            Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => out_tail.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join("\n"),
        };
        let _ = tokio::fs::remove_file(&last_msg_file).await;

        Ok(TaskResult {
            status: if exit.success() { WorkerStatus::Succeeded } else { WorkerStatus::Failed },
            summary,
            artifacts,
            evidence,
            errors: if exit.success() { vec![] } else { vec![format!("进程退出：{exit}")] },
        })
    }
}
