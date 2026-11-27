#!/usr/bin/env python3
"""W4 user-simulation soak driver (#426 / LOCAL_E2E §5).

Drives a "work week" schedule against a running AgentNexus:
  - work hours: ask / delegate / follow-up every 2–5 minutes
  - leave / return + cross-conversation switch
  - memory correct / revoke (best-effort)
  - off hours: only poll tasks / health

Supports compressed clock via --day-minutes (default 480 = 8h real day).
Does not modify crates/ or web/src/.
"""
from __future__ import annotations

import argparse
import json
import random
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path


ASKS = [
    "用一句话说明今天的工作重点是什么？",
    "帮我列三个本周可完成的小目标。",
    "现在几点了？用本机时间回答。",
    "把「记得明天晨会」记成一条偏好以外的事实记忆草稿（若接口可用）。",
]
DELEGATES = [
    "请在工作区创建一个短 Markdown 文件 soak_note_{n}.md，写入一行时间戳与「soak ok」，直接执行。",
    "请用 general 写一个简短 checklist 到 soak_check_{n}.md，直接执行。",
]
FOLLOWUPS = [
    "刚才那个任务现在什么状态？一句话。",
    "如果已经完成，把结果摘要再发我一句。",
]


def req(base: str, method: str, path: str, body=None, timeout=180):
    data = None if body is None else json.dumps(body).encode()
    h = {"Content-Type": "application/json", "Accept": "application/json"}
    r = urllib.request.Request(base + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            j = json.loads(raw)
        except Exception:
            j = {"raw": raw[:1000]}
        return e.code, j
    except Exception as e:
        return None, {"error": str(e)}


@dataclass
class Stats:
    asks: int = 0
    delegates: int = 0
    followups: int = 0
    errors: int = 0
    latencies_ms: list[float] = field(default_factory=list)
    events: list[dict] = field(default_factory=list)

    def log(self, kind: str, **kw):
        row = {"ts": datetime.now(timezone.utc).isoformat(), "kind": kind, **kw}
        self.events.append(row)
        print(json.dumps(row, ensure_ascii=False), flush=True)


def create_conv(base: str, title: str) -> str | None:
    st, c = req(base, "POST", "/api/conversations", {"title": title})
    if st == 200 and isinstance(c, dict):
        return c.get("id")
    return None


def send(base: str, cid: str, text: str, stats: Stats, kind: str):
    t0 = time.time()
    st, body = req(base, "POST", f"/api/conversations/{cid}/messages", {"text": text})
    ms = (time.time() - t0) * 1000
    stats.latencies_ms.append(ms)
    ok = st == 200
    if not ok:
        stats.errors += 1
    stats.log(kind, http=st, ms=round(ms, 1), cid=cid, text=text[:80], ok=ok)
    return st, body


def phase_work(base: str, stats: Stats, convs: list[str], end_at: float, interval_range):
    n = 0
    while time.time() < end_at:
        n += 1
        cid = random.choice(convs)
        roll = random.random()
        if roll < 0.45:
            send(base, cid, random.choice(ASKS), stats, "ask")
            stats.asks += 1
        elif roll < 0.75:
            text = random.choice(DELEGATES).format(n=n)
            send(base, cid, text, stats, "delegate")
            stats.delegates += 1
        else:
            send(base, cid, random.choice(FOLLOWUPS), stats, "followup")
            stats.followups += 1
        # cross-session: occasionally create another conversation
        if n % 7 == 0:
            nid = create_conv(base, f"soak-switch-{n}")
            if nid:
                convs.append(nid)
                stats.log("switch", cid=nid)
        time.sleep(random.uniform(*interval_range))


def phase_away(base: str, stats: Stats, end_at: float):
    while time.time() < end_at:
        st, h = req(base, "GET", "/api/health", timeout=10)
        st2, tasks = req(base, "GET", "/api/tasks", timeout=15)
        n_run = 0
        items = tasks if isinstance(tasks, list) else (tasks or {}).get("tasks") or []
        for t in items:
            if t.get("status") in ("RUNNING", "VERIFYING", "QUEUED"):
                n_run += 1
        stats.log("away_poll", health=st, runningish=n_run)
        if st != 200:
            stats.errors += 1
        time.sleep(60)


def try_memory_ops(base: str, stats: Stats):
    """Best-effort: create style memory then revoke if desktop API present."""
    st, body = req(
        base,
        "POST",
        "/api/desktop/v1/memories",
        {
            "kind": "preference",
            "content": "soak-temp: 回答尽量短。",
            "principal": "local",
        },
        timeout=30,
    )
    stats.log("memory_create", http=st)
    if st not in (200, 201) or not isinstance(body, dict):
        return
    mid = body.get("id") or (body.get("memory") or {}).get("id")
    if not mid:
        return
    st2, _ = req(base, "POST", f"/api/desktop/v1/memories/{mid}/confirm", {}, timeout=30)
    stats.log("memory_confirm", http=st2, id=mid)
    st3, _ = req(base, "POST", f"/api/desktop/v1/memories/{mid}/revoke", {}, timeout=30)
    stats.log("memory_revoke", http=st3, id=mid)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", default="http://127.0.0.1:8787")
    ap.add_argument("--duration-hours", type=float, default=8.0, help="wall-clock length")
    ap.add_argument(
        "--day-minutes",
        type=float,
        default=480.0,
        help="compressed 'work day' length in real minutes (work+away inside one cycle)",
    )
    ap.add_argument("--work-ratio", type=float, default=0.75, help="fraction of day that is work hours")
    ap.add_argument("--min-interval", type=float, default=120.0)
    ap.add_argument("--max-interval", type=float, default=300.0)
    ap.add_argument("--out", type=Path, default=Path("soak-events.jsonl"))
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()
    random.seed(args.seed)

    st, health = req(args.base, "GET", "/api/health", timeout=10)
    if st != 200:
        print(json.dumps({"error": "health_failed", "http": st, "body": health}), flush=True)
        return 1

    stats = Stats()
    convs = []
    for i in range(2):
        cid = create_conv(args.base, f"soak-main-{i}")
        if cid:
            convs.append(cid)
    if not convs:
        print(json.dumps({"error": "no_conversation"}), flush=True)
        return 1

    try_memory_ops(args.base, stats)

    wall_end = time.time() + args.duration_hours * 3600
    day_secs = args.day_minutes * 60.0
    work_secs = day_secs * args.work_ratio
    away_secs = day_secs - work_secs
    interval = (args.min_interval, args.max_interval)

    day = 0
    while time.time() < wall_end:
        day += 1
        stats.log("day_start", day=day)
        work_end = min(time.time() + work_secs, wall_end)
        phase_work(args.base, stats, convs, work_end, interval)
        if time.time() >= wall_end:
            break
        away_end = min(time.time() + away_secs, wall_end)
        phase_away(args.base, stats, away_end)

    # summary
    lats = sorted(stats.latencies_ms)
    p95 = lats[int(0.95 * (len(lats) - 1))] if lats else None
    summary = {
        "asks": stats.asks,
        "delegates": stats.delegates,
        "followups": stats.followups,
        "errors": stats.errors,
        "latency_p95_ms": p95,
        "conversations": convs,
        "days": day,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w") as f:
        for e in stats.events:
            f.write(json.dumps(e, ensure_ascii=False) + "\n")
        f.write(json.dumps({"kind": "summary", **summary}, ensure_ascii=False) + "\n")
    print(json.dumps({"done": True, **summary}, ensure_ascii=False), flush=True)
    return 0 if stats.errors == 0 else 3


if __name__ == "__main__":
    raise SystemExit(main())
