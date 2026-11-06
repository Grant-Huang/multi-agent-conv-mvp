"""ledger_stats.py 的测试：当前状态、当天要核对的条目、周统计、角色越权检查。"""

import json
import subprocess
import sys
from pathlib import Path

import ledger_stats as ls

TOOL = Path(__file__).resolve().parents[1] / "ledger_stats.py"
REPO_LEDGER = Path(__file__).resolve().parents[3] / "docs" / "review" / "findings.jsonl"


def opened(fid, at="2026-11-01", sev="P1"):
    return {"id": fid, "at": at, "by": "reviewer", "event": "opened", "severity": sev,
            "risk": "L2", "file": "a.rs", "line": 1, "title": "t", "verdict": "inferred",
            "evidence": "e", "suggestion": "s", "source": "r"}


def ev(fid, event, by, at="2026-11-02", **extra):
    return {"id": fid, "at": at, "by": by, "event": event, **extra}


def test_current_state_follows_last_status_event_and_ignores_notes():
    events = [
        opened("A"),
        ev("A", "fix_claimed", "fixer", pr="p", commit="c", tests=[]),
        ev("A", "note", "human", text="x"),
        opened("B"),
        ev("B", "verified_fixed", "reviewer", commit="c", evidence="e"),
        opened("C"),
        ev("C", "fix_claimed", "fixer", pr="p", commit="c", tests=[]),
        ev("C", "reopened", "reviewer", evidence="e"),
        opened("D"),
        ev("D", "rejected", "human", reason="r"),
    ]
    items = ls.build_items(events)
    assert {k: v.state for k, v in items.items()} == {
        "A": "fix_claimed", "B": "closed", "C": "open", "D": "closed"}
    assert items["C"].reopen_count == 1


def test_due_on_weekday_is_p0_p1_open_plus_all_fix_claimed():
    events = [
        opened("P0", sev="P0"), opened("P1", sev="P1"), opened("P2", sev="P2"),
        opened("P3", sev="P3"),
        opened("FC", sev="P3"), ev("FC", "fix_claimed", "fixer", pr="p", commit="c", tests=[]),
    ]
    items = ls.build_items(events)
    # 2026-11-07 是周六
    assert ls.due_ids(items, "2026-11-07") == ["P0", "P1", "FC"]


def test_due_on_friday_is_every_open_or_fix_claimed_item():
    events = [opened("P0", sev="P0"), opened("P3", sev="P3"),
              opened("X", sev="P2"), ev("X", "wontfix", "human", reason="r")]
    items = ls.build_items(events)
    # 2026-11-13 是周五；已关闭的 X 不在其中
    assert ls.due_ids(items, "2026-11-13") == ["P0", "P3"]


def test_weekly_metrics_count_events_inside_window():
    events = [
        opened("A", at="2026-11-01"),
        ev("A", "verified_fixed", "reviewer", at="2026-11-05", commit="c", evidence="e"),
        opened("B", at="2026-11-04"),
        ev("B", "fix_claimed", "fixer", at="2026-11-05", pr="p", commit="c", tests=[]),
        ev("B", "reopened", "reviewer", at="2026-11-06", evidence="e"),
        opened("C", at="2026-11-06"),
        ev("C", "rejected", "human", at="2026-11-06", reason="r"),
        opened("OLD", at="2026-10-01"),
    ]
    m = ls.metrics(events, ls.build_items(events), since="2026-11-03", until="2026-11-07")
    assert m["opened"] == 2                  # B、C（A、OLD 在窗口之前）
    assert m["verified_fixed"] == 1
    assert m["reopened"] == 1
    assert m["rejected"] == 1
    assert m["reopen_rate"] == 0.5           # 重开 / (核对通过 + 重开)
    assert m["avg_days_to_fix"] == 4.0       # A：11-01 打开，11-05 核对通过
    assert m["open_now"] == {"P1": 2}        # B 重开后为 open，OLD 仍 open；C 已驳回


def test_role_violations_are_reported():
    events = [
        opened("A"),
        ev("A", "fix_claimed", "reviewer", pr="p", commit="c", tests=[]),   # 评审不能写
        ev("A", "verified_fixed", "fixer", commit="c", evidence="e"),       # 修复不能写
        ev("A", "rejected", "reviewer", reason="r"),                         # 只有人能写
        ev("ZZ", "fix_claimed", "fixer", pr="p", commit="c", tests=[]),     # 没有 opened
    ]
    problems = ls.check(events)
    assert len(problems) == 4
    assert any("ZZ" in p for p in problems)


def test_missing_required_fields_are_reported():
    bad = opened("A")
    del bad["severity"]
    assert ls.check([bad])


def test_cli_reads_jsonl_and_exits_nonzero_on_problems(tmp_path):
    ledger = tmp_path / "f.jsonl"
    rows = [opened("A", sev="P0"), ev("A", "rejected", "fixer", reason="r")]
    ledger.write_text("".join(json.dumps(r) + "\n" for r in rows), encoding="utf-8")
    r = subprocess.run([sys.executable, str(TOOL), "--ledger", str(ledger),
                        "--date", "2026-11-07"], capture_output=True, text=True)
    assert r.returncode == 1
    assert "rejected" in r.stdout


def test_cli_invalid_json_line_is_a_problem(tmp_path):
    ledger = tmp_path / "f.jsonl"
    ledger.write_text('{"id": "A"\n', encoding="utf-8")
    r = subprocess.run([sys.executable, str(TOOL), "--ledger", str(ledger),
                        "--date", "2026-11-07"], capture_output=True, text=True)
    assert r.returncode == 1
    assert "第 1 行" in r.stdout


def test_repository_ledger_is_valid():
    # 仓库里的台账本身必须通过检查：格式错误或越权事件会让 CI 失败。
    events, problems = ls.load(REPO_LEDGER)
    assert problems == []
    assert ls.check(events) == []
