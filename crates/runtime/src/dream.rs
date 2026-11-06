//! 晚间审查：读当天日志里的失败路由，写出草稿。不改评测集，不改技能文件。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

/// 从一行 JSON 日志里取出失败路由的 task_id。普通信息行返回 None。
pub fn failed_task_id(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let task_id = v
        .get("task_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())?;
    let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("");
    let has_error = v
        .get("error")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.is_empty());
    let event = v.get("event").and_then(|x| x.as_str()).unwrap_or("");
    let failed = level.eq_ignore_ascii_case("ERROR") || has_error || event.ends_with("_failed");
    if failed {
        Some(task_id.to_string())
    } else {
        None
    }
}

/// 技能文本里缺了哪几条禁止项。空结果表示文本已经写明。
pub fn skill_gaps(text: &str) -> Vec<&'static str> {
    let mut gaps = Vec::new();
    if !text.contains("空结果") && !text.contains("没有结果") {
        gaps.push("空结果时不要改搜索源");
    }
    if !text.contains("日期") || !text.contains("旧页面") {
        gaps.push("问最近时把日期写进关键词，不要把旧页面当成今天");
    }
    if !text.contains("不要编造") {
        gaps.push("不要编造来源");
    }
    gaps
}

pub fn draft_text(task_ids: &[String], gaps: &[&str]) -> String {
    let mut out =
        String::from("# dream 草稿\n\n这是审查草稿。确认后才可以改评测集或技能文件。\n\n");
    if task_ids.is_empty() {
        out.push_str("## 派发\n没有失败路由。\n");
    } else {
        out.push_str("## 建议补进评测集的失败路由\n");
        for id in task_ids {
            out.push_str(&format!("- task_id={id}\n"));
        }
    }
    out.push_str("\n## 技能说明\n");
    if gaps.is_empty() {
        out.push_str("空结果、日期、不要编造，这三类都已写在技能文本里。\n");
    } else {
        for g in gaps {
            out.push_str(&format!("- 缺少：{g}\n"));
        }
    }
    out
}

/// 读 `logs/agentnexus.jsonl.<date>`。没有失败记录时不创建草稿，返回 false。
pub fn run(log_dir: &Path, date: &str, skill_text: &str, out_dir: &Path) -> Result<bool> {
    let mut ids = Vec::new();
    let log_path = log_dir.join(format!("agentnexus.jsonl.{date}"));
    if log_path.is_file() {
        let text = fs::read_to_string(&log_path)?;
        for line in text.lines() {
            if let Some(id) = failed_task_id(line) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    if ids.is_empty() {
        return Ok(false);
    }
    fs::create_dir_all(out_dir)?;
    let path: PathBuf = out_dir.join("draft.md");
    fs::write(&path, draft_text(&ids, &skill_gaps(skill_text)))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_log_does_not_write_a_draft() {
        let dir = std::env::temp_dir().join(format!("dream-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("logs")).unwrap();
        let out = dir.join("out");
        let wrote = run(&dir.join("logs"), "2026-11-05", "", &out).unwrap();
        assert!(!wrote);
        assert!(!out.join("draft.md").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_route_draft_names_the_task_and_leaves_the_dataset() {
        let dir = std::env::temp_dir().join(format!("dream-hit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            logs.join("agentnexus.jsonl.2026-11-05"),
            "{\"level\":\"ERROR\",\"event\":\"agent.run_failed\",\"task_id\":\"task-9\"}\n{\"level\":\"INFO\",\"event\":\"conversation.turn\",\"task_id\":\"task-ok\"}\n",
        )
        .unwrap();
        let dataset = dir.join("routing.jsonl");
        fs::write(&dataset, "{\"id\":\"keep\"}\n").unwrap();
        let before = fs::read(&dataset).unwrap();
        let out = dir.join("dream");
        assert!(run(
            &logs,
            "2026-11-05",
            "没有结果时换一种说法。把日期写进关键词。不要编造。",
            &out
        )
        .unwrap());
        let draft = fs::read_to_string(out.join("draft.md")).unwrap();
        assert!(draft.contains("task_id=task-9"));
        assert!(!draft.contains("task-ok"));
        assert_eq!(fs::read(&dataset).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shipped_skill_forbids_empty_stale_and_made_up_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let skill =
            fs::read_to_string(root.join("tools/websearch/skills/websearch/SKILL.md")).unwrap();
        assert!(skill_gaps(&skill).is_empty(), "{:?}", skill_gaps(&skill));
        let readme = fs::read_to_string(root.join("tools/websearch/README.md")).unwrap();
        assert!(readme.contains("pi --skill ./tools/websearch/skills/websearch"));
    }
}
