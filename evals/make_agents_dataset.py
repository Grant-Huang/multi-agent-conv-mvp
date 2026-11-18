#!/usr/bin/env python3
"""生成 AG.4"对话里管理助手"的评测集 evals/agents.jsonl（90 条：正例 54、反例 36）。

用法：
    python3 evals/make_agents_dataset.py          # 写出 evals/agents.jsonl
    cargo run --release --bin an-eval -- config.toml evals/agents.jsonl --agent-management --label AG4

门槛（2026-11-17 用户确认）：动作、agent_op、agent_id 都对才算对，整体 ≥ 96%（最多错 3 条）；
同时原有 evals/routing.jsonl 的准确率不得低于打开开关前（同一版本代码、同一模型各跑一次）。

请修改本脚本，而不是直接改 agents.jsonl。只用 Python 标准库。
"""

import json
import os

HEADER = """// AG.4 对话里管理助手的评测集（由 evals/make_agents_dataset.py 生成，请修改脚本而不是直接改这个文件）
// 用法：an-eval <config.toml> evals/agents.jsonl --agent-management
//
// 标注口径：
// - 只有在说"助手本身"时才是 manage_agents：接入 / 接进来（connect）、以后某类任务交给谁（set_default）、
//   停用 / 停掉 / 别再用（disable）、恢复 / 重新启用（enable）、有哪些助手（list）、撤销刚才对助手的修改（undo）
// - agent_id：说了是哪个就填对应 id（cursor / claude / codex-acp / zcode / opencode / hermes / pi / general 等）；没说是哪个填空字符串
// - 让某个助手干活（"用 Cursor 改这个函数"）不是 manage_agents，按原规则交给后台
// - 比较、询问、安装方法、概念类提问是 respond
// - 与进行中任务有关的取消 / 补充是 follow_up
// - 正例 54 条、反例 36 条"""


def pos(id_, text, op, agent_id, tags=(), history=None, note=""):
    """正例：manage_agents"""
    d = {"id": id_, "text": text, "tags": ["agents", "pos", op, *tags],
         "expect_action": "manage_agents", "expect_agent_op": op, "expect_agent_id": agent_id}
    if history:
        d["history"] = history
    if note:
        d["note"] = note
    return d


def neg(id_, text, action, role=None, tags=(), active=None, follow=None, note=""):
    """反例：不是 manage_agents"""
    d = {"id": id_, "text": text, "tags": ["agents", "neg", *tags], "expect_action": action}
    if role:
        d["expect_role"] = role
    if active:
        d["active_tasks"] = active
    if follow:
        d["expect_task_id"], d["expect_follow_up_kind"] = follow
    if note:
        d["note"] = note
    return d


def hist(user, assistant):
    return [{"role": "user", "content": user}, {"role": "assistant", "content": assistant}]


CACHE = {"id": "task-cache", "goal": "给 MES 接口加一层缓存", "role": "coding", "status": "RUNNING"}
WEEKLY = {"id": "task-weekly", "goal": "汇总本周质量数据，生成质量周报保存到 reports/weekly.md",
          "role": "general", "status": "RUNNING"}

POSITIVE = [
    # ───── connect（14）─────
    pos("ag-con-01", "把 Cursor 接进来", "connect", "cursor"),
    pos("ag-con-02", "帮我接入 Claude Code", "connect", "claude"),
    pos("ag-con-03", "接一下 codex 的 ACP 版本", "connect", "codex-acp"),
    pos("ag-con-04", "我装好了 zcode，帮我连上", "connect", "zcode"),
    pos("ag-con-05", "把 opencode 加进来当写代码的助手", "connect", "opencode"),
    pos("ag-con-06", "add cursor agent", "connect", "cursor", tags=("english",)),
    pos("ag-con-07", "接入 claude code 吧", "connect", "claude"),
    pos("ag-con-08", "把 hermes 接进来查资料用", "connect", "hermes"),
    pos("ag-con-09", "帮我接个写代码的助手", "connect", "", tags=("unnamed",)),
    pos("ag-con-10", "再接一个能查资料的助手", "connect", "", tags=("unnamed",)),
    pos("ag-con-11", "curser 能接进来吗，帮我接上", "connect", "cursor", tags=("typo",)),
    pos("ag-con-12", "把 pi 也接进来", "connect", "pi"),
    pos("ag-con-13", "我想用 Cursor，帮我把它配置好", "connect", "cursor", tags=("indirect",)),
    pos("ag-con-14", "把 Claude 的编码助手接进系统", "connect", "claude", tags=("indirect",)),
    # ───── set_default（11）─────
    pos("ag-def-01", "以后写代码都交给 Cursor", "set_default", "cursor"),
    pos("ag-def-02", "默认用 Claude Code 写代码", "set_default", "claude"),
    pos("ag-def-03", "写代码的默认助手换成 opencode", "set_default", "opencode"),
    pos("ag-def-04", "以后查资料用 hermes", "set_default", "hermes"),
    pos("ag-def-05", "把 codex acp 设成默认", "set_default", "codex-acp"),
    pos("ag-def-06", "以后改代码别用 pi 了，换成 cursor", "set_default", "cursor",
        note="说的是以后换谁，不是停用 pi"),
    pos("ag-def-07", "写代码的活默认给 zcode", "set_default", "zcode"),
    pos("ag-def-08", "set claude code as the default coder", "set_default", "claude", tags=("english",)),
    pos("ag-def-09", "把默认的写代码助手改一下", "set_default", "", tags=("unnamed",)),
    pos("ag-def-10", "以后通用任务交给内置的通用执行就行", "set_default", "general"),
    pos("ag-def-11", "默认用 Claude", "set_default", "claude", tags=("short",)),
    # ───── disable（10）─────
    pos("ag-dis-01", "把 zcode 停掉", "disable", "zcode"),
    pos("ag-dis-02", "停用 Cursor", "disable", "cursor"),
    pos("ag-dis-03", "别再用 pi 了", "disable", "pi"),
    pos("ag-dis-04", "暂时禁用 claude code", "disable", "claude"),
    pos("ag-dis-05", "把 opencode 关了", "disable", "opencode"),
    pos("ag-dis-06", "hermes 先别用了", "disable", "hermes"),
    pos("ag-dis-07", "停掉一个写代码的助手", "disable", "", tags=("unnamed",)),
    pos("ag-dis-08", "disable the cursor agent", "disable", "cursor", tags=("english",)),
    pos("ag-dis-09", "codex acp 先停一下", "disable", "codex-acp"),
    pos("ag-dis-10", "把那个 zcode 助手禁掉", "disable", "zcode"),
    # ───── enable（7）─────
    pos("ag-ena-01", "把 cursor 重新启用", "enable", "cursor"),
    pos("ag-ena-02", "恢复 zcode", "enable", "zcode", tags=("short",)),
    pos("ag-ena-03", "pi 再打开吧", "enable", "pi"),
    pos("ag-ena-04", "重新启用 Claude Code", "enable", "claude"),
    pos("ag-ena-05", "把刚才停掉的助手恢复一下", "enable", "", tags=("unnamed",)),
    pos("ag-ena-06", "hermes 恢复使用", "enable", "hermes"),
    pos("ag-ena-07", "re-enable opencode", "enable", "opencode", tags=("english",)),
    # ───── list（7）─────
    pos("ag-lst-01", "现在有哪些助手可以用", "list", ""),
    pos("ag-lst-02", "列一下接入的 agent", "list", ""),
    pos("ag-lst-03", "我能用哪些编码助手", "list", ""),
    pos("ag-lst-04", "看看本机装了哪些助手", "list", ""),
    pos("ag-lst-05", "你后台都有哪些助手在干活", "list", ""),
    pos("ag-lst-06", "show me the agents", "list", "", tags=("english",)),
    pos("ag-lst-07", "现在默认是谁在写代码", "list", "", note="问现状，列出来看"),
    # ───── undo（5，带上一轮对助手的修改）─────
    pos("ag-und-01", "撤销刚才的修改", "undo", "",
        history=hist("把 Cursor 接进来", "已完成：接入「Cursor CLI」，以后可以把「写代码」类的任务交给它")),
    pos("ag-und-02", "刚才那个改回去", "undo", "",
        history=hist("以后写代码交给 zcode", "已完成：以后「写代码」类的任务交给 zcode")),
    pos("ag-und-03", "不对，撤回上一次对助手的修改", "undo", "",
        history=hist("把 zcode 停掉", "已完成：停用「ZCode」：之后不再把任务交给它，正在做的任务会照常做完")),
    pos("ag-und-04", "undo the last agent change", "undo", "", tags=("english",),
        history=hist("disable cursor", "已完成：停用「Cursor CLI」")),
    pos("ag-und-05", "刚才默认设错了，恢复原样", "undo", "",
        history=hist("默认用 Claude Code 写代码", "已完成：以后「写代码」类的任务交给 claude")),
]

NEGATIVE = [
    # ───── 让助手干活：交给后台（8）─────
    neg("ag-neg-01", "用 Cursor 帮我改一下 src/main.rs 里的函数", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-02", "让 Claude Code 修复登录接口偶尔返回 500 的问题", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-03", "用 opencode 跑一下项目的测试", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-04", "codex 帮我在仓库里写个斐波那契函数", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-05", "叫 zcode 重构 utils.py", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-06", "用 pi 给 order 模块补单元测试", "delegate", "coding", tags=("use-agent",)),
    neg("ag-neg-07", "让 hermes 写一份本周质量周报并保存", "delegate", "general", tags=("use-agent",)),
    neg("ag-neg-08", "让助手在仓库里写个脚本，把 data 下的 CSV 转成 JSON", "delegate", "coding",
        tags=("use-agent",)),
    # ───── 提问、比较、安装方法：直接回答（12）─────
    neg("ag-neg-09", "Cursor 和 Claude Code 哪个好", "respond", tags=("ask",)),
    neg("ag-neg-10", "Cursor 怎么安装", "respond", tags=("ask",), note="问安装方法，回答即可"),
    neg("ag-neg-11", "什么是 ACP 协议", "respond", tags=("ask",)),
    neg("ag-neg-12", "opencode 是谁开发的", "respond", tags=("ask",)),
    neg("ag-neg-13", "zcode 支持哪些模型", "respond", tags=("ask",)),
    neg("ag-neg-14", "Claude Code 收费吗", "respond", tags=("ask",)),
    neg("ag-neg-15", "助手和 agent 有什么区别", "respond", tags=("ask",)),
    neg("ag-neg-16", "codex-acp 和 codex 命令行有什么不同", "respond", tags=("ask",)),
    neg("ag-neg-17", "你是用什么模型做判断的", "respond", tags=("ask",)),
    neg("ag-neg-18", "为什么要停用不用的助手", "respond", tags=("ask",)),
    neg("ag-neg-19", "我该选哪个编码助手？说说你的建议", "respond", tags=("ask",)),
    neg("ag-neg-20", "hermes 适合做什么", "respond", tags=("ask",)),
    # ───── 进行中任务的取消 / 补充（3）─────
    neg("ag-neg-21", "先停一下吧，这个不用做了", "follow_up", tags=("follow",), active=[CACHE],
        follow=("task-cache", "cancel"), note="停的是任务，不是助手"),
    neg("ag-neg-22", "把这个任务撤销掉", "follow_up", tags=("follow",), active=[WEEKLY],
        follow=("task-weekly", "cancel"), note="撤销的是任务，不是对助手的修改"),
    neg("ag-neg-23", "缓存过期时间改成 10 分钟", "follow_up", tags=("follow",), active=[CACHE],
        follow=("task-cache", "supplement")),
    # ───── 其他常见请求（13）─────
    neg("ag-neg-24", "帮我算一下 (12+8)/4", "delegate", "general"),
    neg("ag-neg-25", "现在几点", "delegate", "general"),
    neg("ag-neg-26", "写一份关于编码助手选型的报告，保存成 Markdown", "delegate", "general"),
    neg("ag-neg-27", "读一下 notes/agents.md，告诉我里面列了哪些助手", "delegate", "general",
        note="读文件，不是查看本机助手"),
    neg("ag-neg-28", "帮我处理一下", "clarify"),
    neg("ag-neg-29", "你好", "respond"),
    neg("ag-neg-30", "谢谢", "respond"),
    neg("ag-neg-31", "把 Cursor 的使用心得整理成文档保存下来", "delegate", "general"),
    neg("ag-neg-32", "用 Claude Code 看看仓库里 src/order.rs 有没有并发问题", "delegate", "coding",
        tags=("use-agent",)),
    neg("ag-neg-33", "在仓库里写个脚本，检查本机装了哪些命令行工具", "delegate", "coding"),
    neg("ag-neg-34", "删除工作目录 tmp 下的日志文件", "delegate", "general"),
    neg("ag-neg-35", "解释一下怎么把一个 agent 接入系统", "respond", tags=("ask",)),
    neg("ag-neg-36", "Cursor 有哪些常用快捷键", "respond", tags=("ask",)),
]


def main():
    assert len(POSITIVE) == 54, len(POSITIVE)
    assert len(NEGATIVE) == 36, len(NEGATIVE)
    ids = [c["id"] for c in POSITIVE + NEGATIVE]
    assert len(ids) == len(set(ids)), "id 重复"
    out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "agents.jsonl")
    with open(out, "w", encoding="utf-8") as f:
        f.write(HEADER + "\n")
        for c in POSITIVE + NEGATIVE:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")
    print(f"写出 {out}：正例 {len(POSITIVE)}，反例 {len(NEGATIVE)}")


if __name__ == "__main__":
    main()
