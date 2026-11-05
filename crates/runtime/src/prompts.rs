//! Conversation 用到的提示词，集中放在这里，方便修改和评测对照。
//!
//! 修改提示词后，请用 `an-eval` 跑一遍评测集（见 evals/EVAL_PROMPT.md），对比改动前后的准确率。

use an_core::{truncate_chars, TaskRecord};
use serde::{Deserialize, Serialize};

/// 用户主动提出的"审查 / 评估 / 检查"类请求怎么处理（配置 `[conversation] review_requests`）。
///
/// 注意：这里只管**用户发起的**审查请求；任务执行后的自动质检由 Policy 决定，与此无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode {
    /// 一律交给独立的 Reviewer（有记录、有证据，但多一轮后台任务）
    #[default]
    Reviewer,
    /// 对话中直接给出的内容由前台直接审查回答；需要读文件或仓库时仍交给后台
    Direct,
    /// 短内容直接回答，长内容、文件、仓库交给后台
    Auto,
}

impl ReviewMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewMode::Reviewer => "reviewer",
            ReviewMode::Direct => "direct",
            ReviewMode::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "reviewer" => Some(Self::Reviewer),
            "direct" => Some(Self::Direct),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// 生成 Turn Controller 提示词需要的信息
#[derive(Debug, Clone)]
pub struct TurnPromptInfo {
    /// General 可用工具清单（每行一个："- 名称：说明"）
    pub tools: String,
    pub review_mode: ReviewMode,
}

/// Turn Controller 的系统提示词。
///
/// 设计要点（来自本地评测的错判分析）：
/// - 规则按优先级排列，"命中第一条即停止"，避免"能直接回答"和"应该委派"同时命中时模型自由发挥；
/// - 把工具清单告诉 Turn Controller，否则它不知道"现在几点"可以交给 current_time；
/// - 明确系统有默认工作目录和代码仓库，避免为路径、测试命令等可自行确定的细节追问；
/// - 这一步**不生成回答内容**，只输出动作，回答在下一步单独生成（降低延迟）。
pub fn turn_system_prompt(info: &TurnPromptInfo, active: &[TaskRecord]) -> String {
    let tasks = if active.is_empty() {
        "无".to_string()
    } else {
        active
            .iter()
            .map(|t| format!("- task_id={}，状态={}，角色={}，目标：{}", t.id, t.status.as_str(), t.role.as_str(), truncate_chars(&t.goal, 120)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let (reviewer_desc, review_rule, respond_review) = match info.review_mode {
        ReviewMode::Reviewer => (
            "独立审查：只读，检查用户给出的内容（代码、SQL、方案、文档、需求）是否正确完整，给出问题清单和评价",
            "   e. 用户要求审查、评估、检查、评审、挑错某段内容、方案、代码或文档 → 交给独立审查（前台不自己审查）\n",
            "",
        ),
        ReviewMode::Direct => (
            "独立审查（当前配置：用户的审查请求由你直接回答，不使用独立审查）",
            "",
            "   - 用户要求审查、评估、检查对话中直接给出的内容（代码片段、SQL、方案、需求描述）——由你直接给出审查意见；\n     需要读取文件或代码仓库才能审查的，按第 2 条 c / d 处理\n",
        ),
        ReviewMode::Auto => (
            "独立审查：只读，检查较长的内容、文件或代码改动，给出问题清单和评价",
            "   e. 审查、评估、检查类请求中：审查对象需要读取文件或代码仓库，或用户贴出的待审查内容较长（超过约 30 行或 1500 字）→ 交给后台\n",
            "   - 审查、评估、检查对话中直接给出的较短内容（几行代码、一条 SQL、一段描述）——由你直接给出审查意见\n",
        ),
    };

    format!(
        "你是 AgentNexus 的会话负责人（前台）。你只负责一件事：针对用户的最新消息，决定这一轮的 action。\n\
         这一步不需要写出回答内容。如果决定直接回答（respond），系统会在下一步单独生成回答。\n\
         \n\
         ## 后台能力\n\
         - general（通用执行）可以调用这些工具：\n{tools}\n\
         \u{20} 文件工具只能访问系统配置的工作目录。用户说\"文件\"\"目录\"\"报告\"时指这里，不必追问路径。\n\
         - coding（代码开发）：在系统配置的默认代码仓库中阅读、修改代码、运行测试。用户提到\"项目\"\"仓库\"\"代码\"或某个源码文件时，\n\
         \u{20} 都指这个仓库，不必追问仓库路径、测试命令、语言版本。\n\
         - reviewer（{reviewer_desc}）\n\
         \n\
         ## 决策规则（从上到下判断，命中第一条即停止）\n\
         1. follow_up：用户的消息是在补充、修改或取消下面\"进行中的任务\"之一。与进行中任务无关的新请求不属于这一条。\n\
         2. delegate（必须交给后台，即使你自己会做也不能直接回答）：\n\
         \u{20}  a. 需要精确数值计算（四则运算、百分比、单位换算、统计）→ general 的 calculator\n\
         \u{20}  b. 需要当前日期、时间、星期 → general 的 current_time\n\
         \u{20}  c. 需要列出、读取、写入工作目录中的文件，或要求生成文档、报告并保存\n\
         \u{20}  d. 需要改动代码仓库：写代码到仓库、修 bug、重构、加测试、跑测试\n\
         {review_rule}\
         3. clarify：用户想让你做事，但缺少无法自行确定的关键信息（例如不知道要处理哪个对象，或有两种完全不同的理解）。\n\
         \u{20}  执行者可以自行确定的细节（文件路径、输出格式、测试命令、代码风格）不算缺少信息，按第 2 条处理。\n\
         4. respond：其余情况，包括：\n\
         \u{20}  - 闲聊、问候、感谢；\n\
         \u{20}  - 常识、概念解释、原理说明；\n\
         \u{20}  - 建议、讨论、方案思路；\n\
         \u{20}  - 在对话中给出示例代码片段（不写入仓库）；\n\
         {respond_review}\
         \n\
         ## 字段要求\n\
         - delegate：task_goal 写成完整、可独立执行的任务描述，包含对话中的必要细节（执行者看不到对话）；用户贴出的待处理内容要原样放进 task_goal。\n\
         - clarify：question 只问最关键的一个问题。\n\
         - follow_up：填写 task_id、follow_up_kind（supplement 或 cancel），补充要求写在 change。\n\
         - 不适用的字段填空字符串。\n\
         \n\
         ## 进行中的任务\n{tasks}",
        tools = info.tools,
    )
}

/// 直接回答（respond）时的系统提示词
pub fn reply_system_prompt(review_mode: ReviewMode) -> String {
    let review = if review_mode == ReviewMode::Reviewer {
        ""
    } else {
        "- 如果用户要求审查对话中的内容，逐条列出问题和修改建议，最后给出总体评价；\n"
    };
    format!(
        "你是 AgentNexus 的助手，直接回答用户的最新消息。\n\
         要求：\n\
         - 使用中文，简洁、准确，先给结论；\n\
         - 不知道就说不知道；不要编造需要工具才能获得的信息（当前时间、文件内容、代码仓库状态、精确计算结果）；\n\
         {review}\
         - 给出代码时使用 Markdown 代码块。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(mode: ReviewMode) -> TurnPromptInfo {
        TurnPromptInfo { tools: "- calculator：计算\n- current_time：时间".into(), review_mode: mode }
    }

    #[test]
    fn prompt_includes_tools_and_mode_specific_rules() {
        let p = turn_system_prompt(&info(ReviewMode::Reviewer), &[]);
        assert!(p.contains("current_time") && p.contains("calculator"));
        assert!(p.contains("交给独立审查（前台不自己审查）"));
        assert!(p.contains("不必追问仓库路径"));

        let p = turn_system_prompt(&info(ReviewMode::Direct), &[]);
        assert!(!p.contains("交给独立审查（前台不自己审查）"));
        assert!(p.contains("由你直接给出审查意见"));

        let p = turn_system_prompt(&info(ReviewMode::Auto), &[]);
        assert!(p.contains("超过约 30 行"));
    }

    #[test]
    fn review_mode_roundtrip() {
        for m in [ReviewMode::Reviewer, ReviewMode::Direct, ReviewMode::Auto] {
            assert_eq!(ReviewMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(ReviewMode::parse("x"), None);
    }
}
