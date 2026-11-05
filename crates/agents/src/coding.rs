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

use an_core::{truncate_chars, codes, AgentAdapter, AgentCard, AgentError, EventSink, Evidence, Role, TaskResult, TaskSpec, WorkerStatus};

use crate::util::{collect_git_changes, expand_tilde, resolve_command};
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
    /// 同时最多执行几个任务（多个任务共用一个工作目录时，建议保持 1）
    #[serde(default = "default_concurrency")]
    pub max_concurrent: u32,
}

fn default_concurrency() -> u32 {
    1
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
        Self {
            command: default_command(),
            args: default_args(),
            workdir: default_workdir(),
            timeout_secs: default_timeout(),
            max_concurrent: default_concurrency(),
        }
    }
}

pub struct CodingAgent {
    cfg: CodingConfig,
    card: AgentCard,
}

/// 保留的输出行数（用于摘要和证据）
const TAIL_LINES: usize = 40;

impl CodingAgent {
    /// 创建时检查命令是否可用；不可用时 Agent 仍然注册，但标记为 disabled，Router 不会选它
    pub fn new(mut cfg: CodingConfig) -> Self {
        cfg.workdir = PathBuf::from(expand_tilde(&cfg.workdir.to_string_lossy()));
        // 只检查命令是否存在，不运行它
        let (enabled, disabled_reason) = match resolve_command(&cfg.command) {
            Some(path) => {
                cfg.command = path.to_string_lossy().into_owned();
                (true, None)
            }
            None => (false, Some(format!("找不到命令 `{}`", cfg.command))),
        };
        let card = AgentCard {
            id: "codex".into(),
            kind: "cli".into(),
            agent_type: "codex".into(),
            location: "本机".into(),
            max_concurrent: cfg.max_concurrent.max(1),
            role: Role::Coding,
            name: "Codex CLI".into(),
            description: Role::Coding.description().into(),
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

#[async_trait::async_trait]
impl AgentAdapter for CodingAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        if !self.card.enabled {
            return Err(AgentError::new(codes::AGENT_NOT_INSTALLED, self.card.disabled_reason.clone().unwrap_or_default()).into());
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
            .map_err(|e| AgentError::new(codes::AGENT_SPAWN, format!("无法启动 {}：{e}", self.cfg.command)))?;

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
            Err(_) => return Ok(TaskResult::failed_code(codes::AGENT_TIMEOUT, format!("Codex 超时（{} 秒）", self.cfg.timeout_secs))),
        };

        // 4. 收集结果与证据
        let mut evidence = vec![Evidence { kind: "exit_code".into(), content: format!("{exit}") }];
        let stdout_tail = out_tail.iter().cloned().collect::<Vec<_>>().join("\n");
        evidence.push(Evidence { kind: "stdout_tail".into(), content: truncate_chars(&stdout_tail, 3000) });
        if !exit.success() && !err_tail.is_empty() {
            let s = err_tail.iter().cloned().collect::<Vec<_>>().join("\n");
            evidence.push(Evidence { kind: "stderr_tail".into(), content: truncate_chars(&s, 3000) });
        }

        let (git_evidence, artifacts) = collect_git_changes(&workdir).await;
        evidence.extend(git_evidence);

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
            errors: if exit.success() { vec![] } else { vec![format!("[{}] 进程退出：{exit}", codes::AGENT_RUNTIME)] },
        })
    }
}
