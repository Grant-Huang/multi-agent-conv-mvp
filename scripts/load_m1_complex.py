"""15 个复杂任务同时发出：看接口响应、排队等待，以及结束后槽位是否还占着。

结果写到 .tmp/load_m1_complex.json。不重启 8200。
"""
import json
import threading
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

BASE = "http://127.0.0.1:8787"
LLAMA = "http://127.0.0.1:8200"
ROOT = Path(__file__).resolve().parents[1]
FRONT_SLOTS = {0, 1}
BG_ROLES = {"general", "reviewer", "coding"}

DOCS = [
    "请写一份 SMT 产线换型操作说明，保存成 Markdown，大约 600 字，包含步骤和注意事项。",
    "请写一份本地模型槽位怎么分给前台和后台的说明，保存成 Markdown，大约 600 字。",
    "请写一份超时以后为什么要退避再重试的说明，保存成 Markdown，大约 600 字。",
    "请写一份任务失败以后如何把错误码写进日志的说明，保存成 Markdown，大约 600 字。",
    "请写一份审查结论分为通过、退回、无法判断的说明，保存成 Markdown，大约 600 字。",
    "请写一份为什么删除和对外发送要人工确认的说明，保存成 Markdown，大约 600 字。",
    "请写一份上下文太长时先裁旧消息再重试的说明，保存成 Markdown，大约 600 字。",
    "请写一份同一会话的消息要排队、不同会话可以并行的说明，保存成 Markdown，大约 600 字。",
    "请写一份工具调用失败时要原样报告、不要换一个工具假装成功的说明，保存成 Markdown，大约 600 字。",
    "请写一份代码任务和文档任务为什么交给不同角色的说明，保存成 Markdown，大约 600 字。",
    "请写一份健康检查失败时前台应该怎么告诉用户的说明，保存成 Markdown，大约 600 字。",
    "请写一份进度事件为什么要合并后再推给界面的说明，保存成 Markdown，大约 600 字。",
    "请写一份搜索没有结果时换关键词而不是换来源的说明，保存成 Markdown，大约 600 字。",
    "请写一份纠错记录只放进提示词、不改当时那次派发的说明，保存成 Markdown，大约 600 字。",
    "请写一份晚上审查只写草稿、不改评测集的说明，保存成 Markdown，大约 600 字。",
]
CODING = [
    "在代码仓库里新增 src/retry_budget.py：实现一个按次数退避的等待函数，并写测试。不要改仓库里已有文件。",
    "在代码仓库里新增 src/slot_lease.py：用一个类记录槽位占用和归还，并写测试。不要改仓库里已有文件。",
    "在代码仓库里新增 src/event_batch.py：把多行日志按条数合并，并写测试。不要改仓库里已有文件。",
    "在代码仓库里新增 src/path_guard.py：拒绝跳出工作目录的路径，并写测试。不要改仓库里已有文件。",
    "在代码仓库里新增 src/deadline.py：计算剩余时间，超时返回错误而不是继续等，并写测试。不要改仓库里已有文件。",
    "在代码仓库里新增 src/draft_only.py：把审查意见写到草稿文件，不覆盖正式文件，并写测试。不要改仓库里已有文件。",
]
TASKS = DOCS[:8] + CODING
N = len(TASKS)


def req(method, url, body=None, timeout=300):
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        r.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


def percentile(values, p):
    if not values:
        return None
    s = sorted(values)
    return s[round((len(s) - 1) * p)]


def create(title):
    return req("POST", f"{BASE}/api/conversations", {"title": title})["id"]


def send(cid, text, out, key):
    t0 = time.time()
    try:
        body = req("POST", f"{BASE}/api/conversations/{cid}/messages", {"text": text}, timeout=300)
        action = (body.get("decision") or {}).get("action")
        task = body.get("task") or {}
        out[key] = {"ok": True, "ms": int((time.time() - t0) * 1000), "action": action, "task_id": task.get("id"), "error": None}
    except Exception as e:  # noqa: BLE001
        out[key] = {"ok": False, "ms": int((time.time() - t0) * 1000), "action": None, "task_id": None, "error": str(e)}


def llama_busy():
    try:
        raw = req("GET", f"{LLAMA}/slots", timeout=5)
    except Exception:  # noqa: BLE001
        return {}
    return {s.get("id"): bool(s.get("is_processing")) for s in raw}


def main():
    day = datetime.now().strftime("%Y-%m-%d")
    path = ROOT / "logs" / f"agentnexus.jsonl.{day}"
    offset = path.stat().st_size if path.exists() else 0
    ids = [create(f"复杂压测{i}") for i in range(N)]
    out = {}
    threads = []
    t0 = time.time()
    for i, (cid, text) in enumerate(zip(ids, TASKS)):
        th = threading.Thread(target=send, args=(cid, text, out, i), daemon=True)
        threads.append(th)
        th.start()
    for th in threads:
        th.join()
    http_done = time.time()

    terminal = {"COMPLETED", "FAILED", "CANCELLED", "NEEDS_HUMAN"}
    states = []
    deadline = time.time() + 900
    while time.time() < deadline:
        states = []
        for cid in ids:
            try:
                states.extend(t.get("status") for t in req("GET", f"{BASE}/api/tasks?conversation_id={cid}", timeout=8))
            except Exception:  # noqa: BLE001
                states.append("error")
        if len(states) >= N and all(s in terminal for s in states):
            break
        time.sleep(2)
    tasks_done = time.time()
    time.sleep(5)
    busy = llama_busy()

    lines = []
    if path.exists():
        with path.open() as f:
            f.seek(offset)
            for line in f:
                try:
                    lines.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
    start_iso = datetime.fromtimestamp(t0, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
    waits = []
    retries = []
    timeouts = 0
    errors = []
    for ev in lines:
        if ev.get("timestamp", "") < start_iso:
            continue
        if ev.get("event") == "llm.call":
            waited = int(ev.get("queue_wait_ms") or 0)
            waits.append({"role": ev.get("role"), "schema": ev.get("schema"), "queue_wait_ms": waited, "total_ms": ev.get("total_ms"), "outcome": ev.get("outcome")})
            if "timeout" in str(ev.get("outcome")):
                timeouts += 1
            if ev.get("outcome") not in (None, "ok"):
                errors.append({"role": ev.get("role"), "outcome": ev.get("outcome"), "code": ev.get("code")})
        if ev.get("event") == "llm.retry":
            retries.append({"role": ev.get("role"), "reason": ev.get("reason"), "backoff_ms": ev.get("backoff_ms")})

    http_ms = [v["ms"] for v in out.values()]
    summary = {
        "n": N,
        "http_wall_s": round(http_done - t0, 1),
        "http_p50_ms": percentile(http_ms, 0.5),
        "http_p95_ms": percentile(http_ms, 0.95),
        "http_max_ms": max(http_ms) if http_ms else None,
        "http_errors": [v for v in out.values() if not v.get("ok")],
        "actions": {},
        "task_wall_s": round(tasks_done - t0, 1),
        "task_status": {s: states.count(s) for s in sorted(set(states))},
        "still_running": [s for s in states if s not in terminal],
        "queue_wait_max_ms": max((w["queue_wait_ms"] for w in waits), default=None),
        "queue_wait_p95_ms": percentile([w["queue_wait_ms"] for w in waits], 0.95),
        "timeouts": timeouts,
        "retries": retries,
        "call_errors": errors,
        "llama_busy_5s_after": busy,
        "zombie_slots": [i for i, on in busy.items() if on],
    }
    for v in out.values():
        summary["actions"][str(v.get("action"))] = summary["actions"].get(str(v.get("action")), 0) + 1
    dest = ROOT / ".tmp" / "load_m1_complex.json"
    dest.parent.mkdir(exist_ok=True)
    dest.write_text(json.dumps({"summary": summary, "calls": waits, "http": out}, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
