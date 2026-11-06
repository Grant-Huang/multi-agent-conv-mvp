"""评审台账（docs/review/findings.jsonl）的统计与检查。

用法（在仓库根目录运行）：
    python3 tools/ci/ledger_stats.py [--ledger PATH] [--date YYYY-MM-DD]
                                     [--since YYYY-MM-DD] [--json]

输出三部分：
1. 今天要核对的条目（due）：平日 = P0/P1 中仍为 open 的 + 所有 fix_claimed；周五 = 全部 open 和 fix_claimed。
   评审 agent 按这个清单核对台账，避免每晚把几十条都过一遍而超时（REVIEW.md 第 5.2 节）。
2. 统计窗口内（默认最近 7 天）的指标：新开、核对通过、重开、驳回、重开率、平均修复天数、当前 open 分布。
   周检用它判断“夜间评审 + 修复”这个循环是否在变好（7-fence-review.md 第 2.5 节）。
3. 格式与越权检查：JSON 是否合法、必需字段是否齐全、事件是否由有权的角色写入（REVIEW.md 第 5.1 节）。

退出码：0 = 检查通过；1 = 发现格式或越权问题；2 = 运行错误（例如文件不存在）。
只使用 Python 标准库。
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

DEFAULT_LEDGER = Path(__file__).resolve().parents[2] / "docs" / "review" / "findings.jsonl"
TZ = ZoneInfo("America/Los_Angeles")  # 与评审指令一致：日期按洛杉矶时区计算

# 每种事件：允许写入的角色（human 可以写任何事件）、必需字段、写入后的状态（None 表示不改变状态）。
EVENT_RULES: dict[str, tuple[set[str], tuple[str, ...], str | None]] = {
    "opened": ({"reviewer"}, ("severity", "risk", "file", "line", "title", "verdict",
                              "evidence", "suggestion", "source"), "open"),
    "fix_claimed": ({"fixer"}, ("pr", "commit", "tests"), "fix_claimed"),
    "verified_fixed": ({"reviewer"}, ("commit", "evidence"), "closed"),
    "reopened": ({"reviewer"}, ("evidence",), "open"),
    "wontfix": (set(), ("reason",), "closed"),
    "rejected": (set(), ("reason",), "closed"),
    "note": ({"reviewer", "fixer"}, ("text",), None),
}
COMMON_FIELDS = ("id", "at", "by", "event")
ROLES = {"reviewer", "fixer", "human"}
HIGH_SEVERITY = {"P0", "P1"}


@dataclass
class Item:
    """一个问题的当前视图（由事件流推导，不单独存储）。"""
    id: str
    severity: str
    opened_at: str
    state: str = "open"
    reopen_count: int = 0
    events: list[dict] = field(default_factory=list)


def load(path: Path) -> tuple[list[dict], list[str]]:
    """逐行读取台账。返回 (事件列表, 解析问题)。空行忽略。"""
    events: list[dict] = []
    problems: list[str] = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError as e:
            problems.append(f"第 {n} 行不是合法 JSON：{e.msg}")
            continue
        if not isinstance(obj, dict):
            problems.append(f"第 {n} 行不是 JSON 对象")
            continue
        events.append(obj)
    return events, problems


def build_items(events: list[dict]) -> dict[str, Item]:
    """按事件顺序推导每个问题的当前状态。保留首次出现的顺序。"""
    items: dict[str, Item] = {}
    for e in events:
        kind = e.get("event")
        if kind == "opened" and e.get("id") not in items:
            items[e["id"]] = Item(e["id"], e.get("severity", ""), e.get("at", ""))
        item = items.get(e.get("id"))
        if item is None or kind not in EVENT_RULES:
            continue
        item.events.append(e)
        new_state = EVENT_RULES[kind][2]
        if new_state is not None and kind != "opened":
            item.state = new_state
            if kind == "reopened":
                item.reopen_count += 1
    return items


def due_ids(items: dict[str, Item], today: str) -> list[str]:
    """今天要核对的条目，顺序与台账中首次出现的顺序一致。"""
    is_friday = date.fromisoformat(today).weekday() == 4
    out = []
    for item in items.values():
        if item.state == "fix_claimed":
            out.append(item.id)
        elif item.state == "open" and (is_friday or item.severity in HIGH_SEVERITY):
            out.append(item.id)
    return out


def metrics(events: list[dict], items: dict[str, Item], since: str, until: str) -> dict:
    """统计 [since, until] 闭区间内的事件。日期都是 YYYY-MM-DD 字符串，可以直接比较。"""
    window = [e for e in events if since <= e.get("at", "") <= until]
    count = {k: sum(1 for e in window if e.get("event") == k)
             for k in ("opened", "fix_claimed", "verified_fixed", "reopened",
                       "rejected", "wontfix")}
    judged = count["verified_fixed"] + count["reopened"]
    days = []
    for e in window:
        if e.get("event") == "verified_fixed" and e.get("id") in items:
            start = date.fromisoformat(items[e["id"]].opened_at)
            days.append((date.fromisoformat(e["at"]) - start).days)
    open_now: dict[str, int] = {}
    for item in items.values():
        if item.state == "open":
            open_now[item.severity] = open_now.get(item.severity, 0) + 1
    return {
        **count,
        "reopen_rate": round(count["reopened"] / judged, 2) if judged else None,
        "avg_days_to_fix": round(sum(days) / len(days), 1) if days else None,
        "open_now": dict(sorted(open_now.items())),
        "fix_claimed_now": sum(1 for i in items.values() if i.state == "fix_claimed"),
    }


def check(events: list[dict]) -> list[str]:
    """格式与越权检查。返回问题描述列表，空列表表示通过。"""
    problems: list[str] = []
    seen_opened: set[str] = set()
    for n, e in enumerate(events, start=1):
        where = f"第 {n} 个事件（id={e.get('id')}, event={e.get('event')}）"
        missing = [f for f in COMMON_FIELDS if f not in e]
        kind = e.get("event")
        if kind not in EVENT_RULES:
            problems.append(f"{where}：未知事件类型 {kind}")
            continue
        allowed, required, _ = EVENT_RULES[kind]
        missing += [f for f in required if f not in e]
        if missing:
            problems.append(f"{where}：缺少字段 {', '.join(missing)}")
        by = e.get("by")
        if by not in ROLES:
            problems.append(f"{where}：未知角色 {by}")
        elif by != "human" and by not in allowed:
            problems.append(f"{where}：{by} 无权写入 {kind}")
        try:
            date.fromisoformat(str(e.get("at")))
        except ValueError:
            problems.append(f"{where}：at 不是 YYYY-MM-DD 日期")
        fid = e.get("id")
        if kind == "opened":
            if fid in seen_opened:
                problems.append(f"{where}：同一个 id 重复 opened")
            seen_opened.add(fid)
        elif kind != "note" and fid not in seen_opened:
            problems.append(f"{where}：id {fid} 之前没有 opened 事件")
    return problems


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--ledger", type=Path, default=DEFAULT_LEDGER)
    ap.add_argument("--date", help="今天的日期，默认按洛杉矶时区取当天")
    ap.add_argument("--since", help="统计窗口起点，默认 --date 往前 6 天（共 7 天）")
    ap.add_argument("--json", action="store_true", help="以 JSON 输出，便于写进报告")
    args = ap.parse_args(argv)

    if not args.ledger.exists():
        print(f"错误：找不到台账 {args.ledger}", file=sys.stderr)
        return 2
    today = args.date or datetime.now(TZ).date().isoformat()
    since = args.since or (date.fromisoformat(today) - timedelta(days=6)).isoformat()

    events, problems = load(args.ledger)
    problems += check(events)
    items = build_items(events)
    result = {
        "date": today,
        "due": due_ids(items, today),
        "metrics": metrics(events, items, since, today),
        "window": [since, today],
        "problems": problems,
    }
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        print(f"今天（{today}）要核对 {len(result['due'])} 条：{', '.join(result['due']) or '无'}")
        print(f"统计窗口 {since} ~ {today}：{json.dumps(result['metrics'], ensure_ascii=False)}")
        for p in problems:
            print(f"问题：{p}")
        print(f"检查结论：{'发现 ' + str(len(problems)) + ' 个问题' if problems else '通过'}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
