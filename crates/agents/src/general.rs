//! General Agent：内置执行，LLM + 工具循环。
//!
//! 没有使用推理服务的原生 function calling，而是让模型每一步输出一个结构化决策：
//! `{"action": "tool_call" | "final", "tool": "...", "args": {...}, "answer": "..."}`。
//! 原因：小模型 + llama.cpp 的原生工具调用依赖 chat template（需要 `--jinja`），
//! 不同模型差异很大；用 JSON Schema 语法约束的方式更稳定，也更容易调试。

use an_core::{AgentAdapter, AgentCard, Artifact, EventSink, Evidence, Role, TaskResult, TaskSpec, WorkerStatus};
use an_llm::{ChatMessage, LlmClient};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::tools::{args_schema, ToolRegistry};

pub struct GeneralAgent {
    llm: LlmClient,
    tools: ToolRegistry,
    /// 最多执行多少步（防止小模型陷入死循环）
    max_steps: usize,
    card: AgentCard,
}

/// 模型每一步的输出（扁平结构，小模型更容易遵守）
#[derive(Debug, Deserialize)]
struct Step {
    /// 简短思考：让小模型先"想一下"再行动，对效果有帮助
    #[serde(default)]
    thought: String,
    action: String,
    #[serde(default)]
    tool: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    answer: String,
}

impl GeneralAgent {
    pub fn new(llm: LlmClient, tools: ToolRegistry, max_steps: usize) -> Self {
        let card = AgentCard {
            role: Role::General,
            name: "General Agent".into(),
            description: "通用执行：知识问答、分析、计算、读写工作目录中的文件、生成 Markdown 文档或报告".into(),
            tools: tools.infos(),
            enabled: true,
            disabled_reason: None,
        };
        Self { llm, tools, max_steps: max_steps.max(1), card }
    }

    fn step_schema(&self) -> Value {
        // 工具名用 enum 约束，模型无法编造不存在的工具；空字符串表示不调用工具
        let mut tool_names: Vec<Value> = self.tools.names().into_iter().map(Value::String).collect();
        tool_names.push(Value::String(String::new()));
        json!({
            "type": "object",
            "properties": {
                "thought": { "type": "string" },
                "action": { "type": "string", "enum": ["tool_call", "final"] },
                "tool": { "type": "string", "enum": tool_names },
                "args": args_schema(),
                "answer": { "type": "string" }
            },
            "required": ["thought", "action", "tool", "args", "answer"]
        })
    }

    fn system_prompt(&self, task: &TaskSpec) -> String {
        format!(
            "你是 AgentNexus 的 General Agent，负责完成后台交给你的任务。\n\
             你一步一步完成任务：每一步要么调用一个工具（action=tool_call），要么给出最终结果（action=final）。\n\
             规则：\n\
             1. 需要数据、计算、读写文件时必须调用工具，不要编造结果；\n\
             2. 调用工具时 answer 留空；给出最终结果时 tool 留空、args 为 {{}}；\n\
             3. 如果任务要求生成文档/报告，先用 write_file 写入文件，再在 final 的 answer 中给出摘要和文件路径；\n\
             4. 最多 {max} 步，信息足够时尽快给出 final。\n\n\
             ## 可用工具\n{tools}\n\n\
             ## 路由建议使用的工具\n{required}\n\n\
             ## 背景信息\n{ctx}",
            max = self.max_steps,
            tools = self.tools.prompt_listing(),
            required = if task.route.required_tools.is_empty() { "无".into() } else { task.route.required_tools.join(", ") },
            ctx = if task.context.is_empty() { "无" } else { &task.context },
        )
    }
}

#[async_trait::async_trait]
impl AgentAdapter for GeneralAgent {
    fn card(&self) -> &AgentCard {
        &self.card
    }

    async fn run(&self, task: TaskSpec, events: EventSink) -> Result<TaskResult> {
        let schema = self.step_schema();
        let mut messages = vec![ChatMessage::system(self.system_prompt(&task)), ChatMessage::user(format!("任务：{}", task.goal))];
        let mut evidence = Vec::new();
        let mut artifacts: Vec<Artifact> = Vec::new();

        for step_no in 1..=self.max_steps {
            // 校验：tool_call 时必须给出工具名
            let step = match self
                .llm
                .chat_json::<Step, _>(&messages, "general_step", &schema, |s| {
                    if s.action == "tool_call" && s.tool.is_empty() {
                        Err("action=tool_call 时必须填写 tool".into())
                    } else if s.action != "tool_call" && s.action != "final" {
                        Err("action 只能是 tool_call 或 final".into())
                    } else {
                        Ok(())
                    }
                })
                .await
            {
                Ok(s) => s.value,
                Err(e) => return Ok(TaskResult { evidence, artifacts, ..TaskResult::failed(e.to_string()) }),
            };

            // 把模型这一步的输出加回对话历史
            messages.push(ChatMessage::assistant(
                json!({"thought": step.thought, "action": step.action, "tool": step.tool, "args": step.args, "answer": step.answer}).to_string(),
            ));

            if step.action == "final" {
                return Ok(TaskResult {
                    status: WorkerStatus::Succeeded,
                    summary: step.answer,
                    artifacts,
                    evidence,
                    errors: vec![],
                });
            }

            // 执行工具
            events.progress(format!("第 {step_no} 步：调用工具 {}，参数 {}", step.tool, an_core::truncate_chars(&step.args.to_string(), 200)));
            let observation = match self.tools.get(&step.tool) {
                None => format!("错误：工具 {} 不存在", step.tool),
                Some(tool) => match tool.call(&step.args).await {
                    Ok(out) => {
                        if let Some(a) = out.artifact {
                            artifacts.push(a);
                        }
                        out.text
                    }
                    Err(e) => format!("工具执行出错：{e:#}"),
                },
            };
            evidence.push(Evidence {
                kind: "tool_call".into(),
                content: format!("{}({}) → {}", step.tool, step.args, an_core::truncate_chars(&observation, 500)),
            });
            messages.push(ChatMessage::user(format!("工具 {} 的返回结果：\n{}", step.tool, observation)));
        }

        Ok(TaskResult {
            evidence,
            artifacts,
            ..TaskResult::failed(format!("超过最大步数 {} 仍未完成", self.max_steps))
        })
    }
}
