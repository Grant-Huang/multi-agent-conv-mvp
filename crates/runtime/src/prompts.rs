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

/// 一条和当前消息相似的纠错，用来放进提示词。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionExample {
    pub user_text: String,
    pub correct_action: String,
    pub correct_role: Option<String>,
    pub note: String,
}

const MAX_CORRECTIONS: usize = 5;
const MAX_EXAMPLE_CHARS: usize = 80;
const MAX_NOTE_CHARS: usize = 40;

/// 有纠正时追加一节；没有时返回空串，调用方拼上去后提示词不变。
/// 最多 5 条。用户原文和说明分别截断，条数再多、单条再长，这一节也不会变长。
pub fn correction_section(examples: &[CorrectionExample]) -> String {
    let shown: Vec<_> = examples.iter().take(MAX_CORRECTIONS).collect();
    if shown.is_empty() {
        return String::new();
    }
    let lines = shown
        .iter()
        .map(|e| {
            let action = match e.correct_role.as_deref() {
                Some(role) if !role.is_empty() => format!("{} / {role}", e.correct_action),
                _ => e.correct_action.clone(),
            };
            let note = if e.note.is_empty() {
                String::new()
            } else {
                format!(" {}", truncate_chars(&e.note, MAX_NOTE_CHARS))
            };
            format!(
                "- 用户：{}。正确做法：{action}。{note}",
                truncate_chars(&e.user_text, MAX_EXAMPLE_CHARS)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n\n## 以往纠正（类似消息应按这里做）\n{lines}")
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
pub fn turn_system_prompt(
    info: &TurnPromptInfo,
    active: &[TaskRecord],
    examples: &[CorrectionExample],
) -> String {
    let tasks = if active.is_empty() {
        "无".to_string()
    } else {
        active
            .iter()
            .map(|t| {
                format!(
                    "- task_id={}，状态={}，角色={}，目标：{}",
                    t.id,
                    t.status.as_str(),
                    t.role.as_str(),
                    truncate_chars(&t.goal, 120)
                )
            })
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
         - coding（代码开发）：在系统配置的默认代码仓库中阅读、修改代码、运行测试；也负责生成网页（HTML/CSS/JS）、脚本、程序等代码产物。\n\
         \u{20} 用户提到\"项目\"\"仓库\"\"代码\"或某个源码文件时，\n\
         \u{20} 都指这个仓库，不必追问仓库路径、测试命令、语言版本。\n\
         - reviewer（{reviewer_desc}）\n\
         \n\
         ## 决策规则（从上到下判断，命中第一条即停止）\n\
         1. follow_up：用户的消息是在补充、修改或取消下面\"进行中的任务\"之一。与进行中任务无关的新请求不属于这一条。\n\
         2. delegate（必须交给后台，即使你自己会做也不能直接回答）：\n\
         \u{20}  a. 需要精确数值计算（四则运算、百分比、单位换算、统计）→ general 的 calculator\n\
         \u{20}  b. 需要当前日期、时间、星期 → general 的 current_time\n\
         \u{20}  c. 需要列出、读取、写入工作目录中的文件，或要求生成 Markdown 或纯文本的文档、报告并保存\n\
         \u{20}  d. 需要改动代码仓库：写代码到仓库、修 bug、重构、加测试、跑测试；\n\
         \u{20}     或要求生成网页（HTML/CSS/JS）、脚本、程序等代码产物（即使用户称之为\"文档\"\"页面\"）→ coding\n\
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
         \u{20} 用户没说保存位置时，不要自行指定保存位置（保存到工作目录还是代码仓库由执行者决定）。\n\
         - clarify：question 只问最关键的一个问题。\n\
         - follow_up：填写 task_id、follow_up_kind（supplement 或 cancel），补充要求写在 change。\n\
         - 不适用的字段填空字符串。\n\
         \n\
         ## 进行中的任务\n{tasks}{corrections}",
        tools = info.tools,
        corrections = correction_section(examples),
    )
}

/// 直接回答（respond）时的系统提示词。
/// 写明系统的实际能力：用户问"你能做什么"时，模型否则会按通用助手的印象编造（例如联网搜索、执行命令）。
pub fn reply_system_prompt(info: &TurnPromptInfo) -> String {
    let review = if info.review_mode == ReviewMode::Reviewer {
        ""
    } else {
        "- 如果用户要求审查对话中的内容，逐条列出问题和修改建议，最后给出总体评价；\n"
    };
    format!(
        "你是 AgentNexus 的助手，直接回答用户的最新消息。\n\
         \n\
         ## 本系统的实际能力\n\
         - 直接对话：回答问题、解释概念、讨论方案、在对话中给出示例代码；\n\
         - 通用执行（后台）：只能使用以下工具：\n{tools}\n\
         \u{20} 文件工具只能访问系统配置的工作目录；\n\
         - 代码开发（后台）：在系统配置的代码仓库中阅读、修改代码、运行测试，生成网页、脚本等代码产物；\n\
         - 独立审查（后台）：检查代码、方案、文档，给出问题清单和评价。\n\
         没有列出的能力系统都不具备（例如工具清单里没有联网工具，就不能联网搜索或打开网页）。\n\
         介绍自己或回答\"你能做什么\"时，只能说以上能力，不要声称可以做到没有列出的事。\n\
         \n\
         要求：\n\
         - 使用中文，简洁、准确，先给结论；\n\
         - 最新消息里有多个问题时，按编号逐条回答，条数与问题数相同，不合并、不遗漏；\n\
         - 不知道就说不知道；不要编造需要工具才能获得的信息（当前时间、文件内容、代码仓库状态、精确计算结果）；\n\
         {review}\
         - 给出代码时使用 Markdown 代码块。",
        tools = info.tools,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(mode: ReviewMode) -> TurnPromptInfo {
        TurnPromptInfo {
            tools: "- calculator：计算\n- current_time：时间".into(),
            review_mode: mode,
        }
    }

    #[test]
    fn prompt_includes_tools_and_mode_specific_rules() {
        let p = turn_system_prompt(&info(ReviewMode::Reviewer), &[], &[]);
        assert!(p.contains("current_time") && p.contains("calculator"));
        assert!(p.contains("交给独立审查（前台不自己审查）"));
        assert!(p.contains("不必追问仓库路径"));

        let p = turn_system_prompt(&info(ReviewMode::Direct), &[], &[]);
        assert!(!p.contains("交给独立审查（前台不自己审查）"));
        assert!(p.contains("由你直接给出审查意见"));

        let p = turn_system_prompt(&info(ReviewMode::Auto), &[], &[]);
        assert!(p.contains("超过约 30 行"));
    }

    #[test]
    fn turn_prompt_separates_code_artifacts_from_documents() {
        let p = turn_system_prompt(&info(ReviewMode::Reviewer), &[], &[]);
        assert!(
            p.contains("网页（HTML/CSS/JS）"),
            "生成网页等代码产物应归入代码开发：{p}"
        );
        assert!(
            p.contains("Markdown 或纯文本"),
            "文档应限定为 Markdown 或纯文本：{p}"
        );
        assert!(
            p.contains("不要自行指定保存位置"),
            "改写任务描述时不应替执行者决定保存位置：{p}"
        );
    }

    #[test]
    fn empty_corrections_leave_the_prompt_unchanged() {
        let bare = turn_system_prompt(&info(ReviewMode::Reviewer), &[], &[]);
        assert!(!bare.contains("以往纠正"));
        let with = turn_system_prompt(
            &info(ReviewMode::Reviewer),
            &[],
            &[CorrectionExample {
                user_text: "斐波那契是什么".into(),
                correct_action: "respond".into(),
                correct_role: None,
                note: "只是在问概念".into(),
            }],
        );
        assert!(with.starts_with(&bare));
        assert!(with.contains("斐波那契是什么"));
        assert!(with.contains("respond"));
    }

    #[test]
    fn correction_section_does_not_grow_with_table_size() {
        let long = "字".repeat(2000);
        let many: Vec<_> = (0..30)
            .map(|_| CorrectionExample {
                user_text: long.clone(),
                correct_action: "delegate".into(),
                correct_role: Some("coding".into()),
                note: long.clone(),
            })
            .collect();
        let section = correction_section(&many);
        let lines = section
            .lines()
            .filter(|l| l.starts_with("- 用户："))
            .count();
        assert_eq!(lines, 5);
        assert!(
            section.chars().count() < 900,
            "纠正一节应有固定上限，实际 {} 字",
            section.chars().count()
        );
        assert!(section.contains('…'));
    }

    #[test]
    fn reply_prompt_lists_real_capabilities_only() {
        let p = reply_system_prompt(&info(ReviewMode::Reviewer));
        assert!(
            p.contains("calculator") && p.contains("current_time"),
            "应列出实际可用的工具：{p}"
        );
        assert!(p.contains("没有列出的能力"), "应禁止声称未列出的能力：{p}");
        assert!(
            !p.contains("如果用户要求审查对话中的内容"),
            "reviewer 模式下前台不自己审查"
        );

        let p = reply_system_prompt(&info(ReviewMode::Direct));
        assert!(p.contains("如果用户要求审查对话中的内容"));
    }

    #[test]
    fn reply_prompt_numbers_each_question() {
        let p = reply_system_prompt(&info(ReviewMode::Reviewer));
        assert!(p.contains("按编号逐条回答"));
        assert!(p.contains("不遗漏"));
    }

    #[test]
    fn review_mode_roundtrip() {
        for m in [ReviewMode::Reviewer, ReviewMode::Direct, ReviewMode::Auto] {
            assert_eq!(ReviewMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(ReviewMode::parse("x"), None);
    }
}
