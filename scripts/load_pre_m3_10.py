"""pre-M3.10 验收：3 个后台任务进行中时，同时打 10 个前台会话。

指标写到标准输出，也写到 .tmp/load_pre_m3_10.json。
前台判断延迟 = conversation 调用的 queue_wait_ms + total_ms（不含后面的直接回答）。
"""
import json
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

BASE = "http://127.0.0.1:8787"
LLAMA = "http://127.0.0.1:8200"
ROOT = Path(__file__).resolve().parents[1]
FRONT_SLOTS = {0, 1}
BG_ROLES = {"general", "reviewer", "coding"}

BG = [
    "请写一份 SMT 产线换型操作说明，保存成 Markdown，大约 800 字，包含步骤和注意事项。",
    "请写一份本地模型槽位怎么分给前台和后台的说明，保存成 Markdown，大约 800 字。",
    "请写一份超时以后为什么要退避再重试的说明，保存成 Markdown，大约 800 字。",
]
FRONT = [
    "你好",
    "早上好",
    "你是谁？",
    "谢谢",
    "今天状态不错",
    "你能做什么？",
    "好的，明白了",
    "辛苦了",
    "哈哈",
    "再见",
]


def req(method, url, body=None, timeout=240):
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(url, data=data, method=method)
    if data is not None:
        r.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


def log_path():
    day = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    return ROOT / "logs" / f"agentnexus.jsonl.{day}"


def percentile(values, p):
    if not values:
        return None
    s = sorted(values)
    idx = round((len(s) - 1) * p)
    return s[idx]


def create(title):
    return req("POST", f"{BASE}/api/conversations", {"title": title})["id"]


def send(cid, text, out, key):
    t0 = time.time()
    try:
        body = req("POST", f"{BASE}/api/conversations/{cid}/messages", {"text": text})
        action = (body.get("decision") or {}).get("action")
        out[key] = {"ok": True, "ms": int((time.time() - t0) * 1000), "action": action, "error": None}
    except Exception as e:  # noqa: BLE001
        out[key] = {"ok": False, "ms": int((time.time() - t0) * 1000), "action": None, "error": str(e)}


def slots_snapshot():
    try:
        diag = req("GET", f"{BASE}/api/diagnostics", timeout=8)
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)}
    backends = diag.get("backends") or []
    local = next((b for b in backends if b.get("name") == "local"), backends[0] if backends else {})
    slots = []
    for s in local.get("slots") or []:
        slots.append({
            "id": s.get("id"),
            "class": s.get("class"),
            "holder": s.get("holder"),
            "server_busy": s.get("server_busy"),
            "tasks": s.get("tasks"),
        })
    calls = local.get("calls") or {}
    return {"slots": slots, "outcomes": calls.get("outcomes") or {}, "calls_total": calls.get("total")}


def llama_busy():
    try:
        raw = req("GET", f"{LLAMA}/slots", timeout=5)
    except Exception:  # noqa: BLE001
        return {}
    return {s.get("id"): bool(s.get("is_processing")) for s in raw}


def main():
    path = log_path()
    offset = path.stat().st_size if path.exists() else 0
    samples = []
    stop = threading.Event()

    def poll():
        while not stop.is_set():
            snap = slots_snapshot()
            snap["llama_busy"] = llama_busy()
            snap["t"] = time.time()
            samples.append(snap)
            stop.wait(1.0)

    bg_ids = [create(f"压测后台{i}") for i in range(3)]
    bg_out = {}
    bg_threads = []
    for i, (cid, text) in enumerate(zip(bg_ids, BG)):
        th = threading.Thread(target=send, args=(cid, text, bg_out, i), daemon=True)
        bg_threads.append(th)
        th.start()

    # 等 3 个后台任务真正占上槽位，再打前台。最多等 90 秒。
    deadline = time.time() + 90
    ready = False
    while time.time() < deadline:
        snap = slots_snapshot()
        busy_bg = [s for s in snap.get("slots") or [] if s.get("server_busy") and s.get("id") not in FRONT_SLOTS]
        holders = [s.get("holder") for s in busy_bg]
        if len(busy_bg) >= 3 or sum(1 for h in holders if h in BG_ROLES) >= 3:
            ready = True
            break
        time.sleep(1)
    if not ready:
        print("警告：90 秒内没有看到 3 个后台槽位在忙，仍开始前台")

    poller = threading.Thread(target=poll, daemon=True)
    poller.start()
    t_front = time.time()
    front_ids = [create(f"压测前台{i}") for i in range(10)]
    front_out = {}
    front_threads = []
    for i, (cid, text) in enumerate(zip(front_ids, FRONT)):
        th = threading.Thread(target=send, args=(cid, text, front_out, i), daemon=True)
        front_threads.append(th)
        th.start()
    for th in front_threads:
        th.join()
    t_front_done = time.time()
    stop.set()
    poller.join(timeout=2)

    # 前台结束后再看一会儿，确认前台槽位没有继续生成
    time.sleep(3)
    after = slots_snapshot()
    after["llama_busy"] = llama_busy()

    for th in bg_threads:
        th.join(timeout=30)
    terminal = {"COMPLETED", "FAILED", "CANCELLED", "NEEDS_HUMAN"}
    bg_states = []
    deadline = time.time() + 240
    while time.time() < deadline:
        bg_states = []
        for cid in bg_ids:
            try:
                bg_states.extend(t.get("status") for t in req("GET", f"{BASE}/api/tasks?conversation_id={cid}", timeout=8))
            except Exception:  # noqa: BLE001
                bg_states.append("error")
        if bg_states and all(s in terminal for s in bg_states):
            break
        time.sleep(2)
    time.sleep(8)
    idle2 = llama_busy()

    lines = []
    if path.exists():
        with path.open() as f:
            f.seek(offset)
            for line in f:
                try:
                    lines.append(json.loads(line))
                except json.JSONDecodeError:
                    pass

    judges = []
    routers = []
    retries = []
    calls = []
    for ev in lines:
        if ev.get("event") == "llm.call" and ev.get("timestamp", "") >= datetime.fromtimestamp(t_front, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S"):
            calls.append(ev)
            waited = int(ev.get("queue_wait_ms") or 0) + int(ev.get("total_ms") or 0)
            # 只要这一轮的动作判断。任务结束后的汇总也是 conversation，但不算判断延迟
            if ev.get("role") == "conversation" and ev.get("schema") == "turn_decision" and ev.get("timestamp", "") >= datetime.fromtimestamp(t_front, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S"):
                judges.append(waited)
            if ev.get("role") == "router":
                routers.append(waited)
        if ev.get("event") == "llm.retry":
            retries.append({"backoff_ms": ev.get("backoff_ms"), "reason": ev.get("reason"), "role": ev.get("role")})

    front_violations = []
    bg_seen = 0
    for snap in samples:
        if snap.get("t", 0) < t_front or snap.get("t", 0) > t_front_done:
            continue
        holders_bg = 0
        for s in snap.get("slots") or []:
            sid = s.get("id")
            holder = s.get("holder")
            if sid in FRONT_SLOTS and holder in BG_ROLES:
                front_violations.append({"slot": sid, "holder": holder})
            if sid not in FRONT_SLOTS and (s.get("server_busy") or holder in BG_ROLES):
                holders_bg += 1
        bg_seen = max(bg_seen, holders_bg)

    immediate = [r for r in retries if (r.get("backoff_ms") or 0) < 400]
    timeout_calls = [c for c in calls if "timeout" in str(c.get("outcome"))]

    summary = {
        "front_wall_s": round(t_front_done - t_front, 1),
        "front": front_out,
        "bg_still_running_at_front_start": ready,
        "judge_n": len(judges),
        "judge_p50_ms": percentile(judges, 0.5),
        "judge_p95_ms": percentile(judges, 0.95),
        "judge_max_ms": max(judges) if judges else None,
        "router_p95_ms": percentile(routers, 0.95),
        "front_actions": {str(v.get("action")): sum(1 for x in front_out.values() if x.get("action") == v.get("action")) for v in front_out.values()},
        "front_errors": [v for v in front_out.values() if not v.get("ok")],
        "bg_slots_peak_during_front": bg_seen,
        "front_slot_background_holders": front_violations,
        "retries": retries,
        "immediate_retries": immediate,
        "timeout_calls": len(timeout_calls),
        "outcomes_after": after.get("outcomes"),
        "slots_3s_after_front": after.get("slots"),
        "llama_busy_3s_after_front": after.get("llama_busy"),
        "bg_final_status": bg_states,
        "llama_busy_8s_after_tasks": idle2,
        "zombie_slots": [i for i, busy in idle2.items() if busy],
    }
    outp = ROOT / ".tmp" / "load_pre_m3_10.json"
    outp.parent.mkdir(exist_ok=True)
    outp.write_text(json.dumps(summary, ensure_ascii=False, indent=2))
    print(json.dumps({k: summary[k] for k in (
        "front_wall_s", "judge_n", "judge_p50_ms", "judge_p95_ms", "judge_max_ms",
        "router_p95_ms", "bg_slots_peak_during_front", "front_slot_background_holders",
        "immediate_retries", "timeout_calls", "outcomes_after", "zombie_slots",
        "bg_still_running_at_front_start", "front_errors",
    )}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
