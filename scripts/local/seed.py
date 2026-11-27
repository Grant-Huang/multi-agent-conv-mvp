#!/usr/bin/env python3
"""本地演示厂数据集（第 6 线 W2 / #425）。

经公开 HTTP API 写入，不直接写库。

子命令：
  create  — 重置记忆夹具后重建 small（或 large 占位）
  reset   — 同 create
  verify  — 按 fixtures/local/expected.<scale>.json 校验

示例：
  python3 scripts/local/seed.py create --base http://127.0.0.1:8787 --scale small
  python3 scripts/local/seed.py verify --base http://127.0.0.1:8787 --scale small
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import date, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIX = ROOT / "fixtures" / "local"


def load_json(name: str):
    return json.loads((FIX / name).read_text(encoding="utf-8"))


def http(method: str, url: str, body: dict | None = None, headers: dict | None = None):
    data = None if body is None else json.dumps(body, ensure_ascii=False).encode()
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("content-type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else {})
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            payload = json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            payload = {"raw": raw}
        return e.code, payload
    except urllib.error.URLError as e:
        return 0, {"error": str(e)}


def hdr(principal: str) -> dict:
    return {"x-desktop-principal": principal}


def ensure_base(base: str) -> bool:
    code, _ = http("GET", f"{base}/api/agents")
    return code == 200


def reset_user_memories(base: str, principal: str) -> None:
    st, _ = http("POST", f"{base}/api/desktop/v1/memories/fixture/reset", {}, hdr(principal))
    assert st == 200, f"memories/fixture/reset: HTTP {st}"


def seed_memories(base: str, principal: str, templates: list, injections: list) -> dict:
    """返回 {key: id}。"""
    ids: dict[str, str] = {}
    for t in templates:
        st, body = http(
            "POST",
            f"{base}/api/desktop/v1/memories",
            {"kind": t["kind"], "title": t["title"], "body": t["body"]},
            hdr(principal),
        )
        mid = (body or {}).get("id")
        if st not in (200, 201) or not mid:
            print(f"  ! memory create fail {principal} {t['key']}: {st} {body}")
            continue
        ids[t["key"]] = mid
        if t.get("confirm"):
            http("POST", f"{base}/api/desktop/v1/memories/{mid}/confirm", {}, hdr(principal))
        if t.get("patch_body"):
            http(
                "PATCH",
                f"{base}/api/desktop/v1/memories/{mid}",
                {"body": t["patch_body"]},
                hdr(principal),
            )
        if t.get("revoke"):
            http("DELETE", f"{base}/api/desktop/v1/memories/{mid}", headers=hdr(principal))

    for inj in injections:
        if inj.get("channel") != "memory":
            continue
        st, body = http(
            "POST",
            f"{base}/api/desktop/v1/memories",
            {"kind": "fact", "title": inj["title"], "body": inj["body"]},
            hdr(principal),
        )
        if st in (200, 201) and body.get("id"):
            ids[inj["id"]] = body["id"]
    return ids


def seed_history_shells(base: str, user: dict, dialogues: dict, days: int) -> list[str]:
    """每人按日建会话壳（标题带日期）；不写历史消息正文（无免 LLM 的公开导入 API）。"""
    role = user["role"]
    titles = dialogues["titles_by_role"].get(role) or ["日常"]
    today = date.today()
    conv_ids = []
    for i in range(days):
        d = today - timedelta(days=days - 1 - i)
        title = f"[local-demo {d.isoformat()}] {titles[i % len(titles)]} · {user['display']}"
        st, body = http("POST", f"{base}/api/conversations", {"title": title})
        if st in (200, 201) and body.get("id"):
            conv_ids.append(body["id"])
    return conv_ids


def seed_live_turns(
    base: str, user: dict, dialogues: dict, injections: list, live_turns: int
) -> tuple[str | None, list[str]]:
    """新建会话 + desktop requests（会走模型）。返回 (conv_id, request_ids)。"""
    st, body = http(
        "POST",
        f"{base}/api/conversations",
        {"title": f"[local-demo live] {user['display']}"},
    )
    if st not in (200, 201) or not body.get("id"):
        print(f"  ! live conv fail {user['principal']}: {st}")
        return None, []
    cid = body["id"]
    prompts = list(dialogues.get("live_prompts") or [])
    for inj in injections:
        if inj.get("channel") == "request":
            prompts.append(inj["text"])
    prompts = prompts[: max(1, live_turns)]
    req_ids = []
    for text in prompts:
        op = f"seed-{user['principal']}-{uuid.uuid4().hex[:10]}"
        st, body = http(
            "POST",
            f"{base}/api/desktop/v1/conversations/{cid}/requests",
            {"client_operation_id": op, "text": text, "scope": "continue"},
            hdr(user["principal"]),
        )
        print(f"  live request {user['principal']}: HTTP {st}")
        if st in (200, 202) and body.get("request_id"):
            req_ids.append(body["request_id"])
        # 给模型一点时间，避免打满
        time.sleep(0.5)
    return cid, req_ids


def seed_tasks(base: str) -> int:
    """桌面快照夹具：多种状态任务。"""
    st, body = http("POST", f"{base}/api/desktop/v1/snapshot/fixture/reset", {})
    assert st == 200, f"snapshot/fixture/reset: HTTP {st}"
    st2, tasks = http("GET", f"{base}/api/tasks")
    items = tasks if isinstance(tasks, list) else (tasks or {}).get("items") or []
    print(f"  tasks_visible={len(items)} (HTTP {st2})")
    return len(items)


def seed_industrial(base: str) -> int:
    st, body = http("POST", f"{base}/api/world/mock/ops/ensure-seed", {})
    print(f"  world/mock/ops/ensure-seed: HTTP {st} body_keys={list((body or {}).keys())[:8]}")
    return st


def seed_introspection(base: str) -> int:
    st, body = http("POST", f"{base}/api/introspection/run", {})
    print(f"  introspection/run: HTTP {st}")
    st2, findings = http("GET", f"{base}/api/introspection/findings")
    items = findings if isinstance(findings, list) else (findings or {}).get("items") or []
    if isinstance(findings, dict) and "findings" in findings:
        items = findings["findings"]
    print(f"  findings_visible≈{len(items) if isinstance(items, list) else findings} (HTTP {st2})")
    return st


def cmd_create(base: str, scale: str, seed: int, live_turns: int, skip_live: bool) -> int:
    if scale == "large":
        print("large：本卡只保证参数可解析；生成大体量未实现（占位）。")
        return 0
    if not ensure_base(base):
        print(f"ERROR: {base} 不可达", file=sys.stderr)
        return 2

    users = load_json("users.json")["users"]
    mem_tpl = load_json("memories.json")["per_user"]
    injections = load_json("injections.json")["samples"]
    dialogues = load_json("dialogues.json")
    days = int(dialogues.get("history_days") or 30)
    ver = (FIX / "VERSION").read_text(encoding="utf-8").strip()
    print(f"create {ver}\n  scale={scale} seed={seed} users={len(users)} days={days} live_turns={live_turns}")

    # 工业 + 任务夹具（全局一次）
    industrial_http = seed_industrial(base)
    task_n = seed_tasks(base)

    summary = {
        "users": [],
        "industrial_http": industrial_http,
        "tasks_visible": task_n,
        "live_skipped": skip_live,
    }

    for u in users:
        print(f"- {u['principal']}")
        reset_user_memories(base, u["principal"])
        mids = seed_memories(base, u["principal"], mem_tpl, injections)
        shells = seed_history_shells(base, u, dialogues, days)
        live_cid, reqs = (None, [])
        if not skip_live and live_turns > 0:
            live_cid, reqs = seed_live_turns(base, u, dialogues, injections, live_turns)
        summary["users"].append(
            {
                "principal": u["principal"],
                "memories": len(mids),
                "history_convs": len(shells),
                "live_conv": live_cid,
                "live_requests": len(reqs),
            }
        )
        print(
            f"  memories={len(mids)} history_convs={len(shells)} live_req={len(reqs)}"
        )

    seed_introspection(base)
    out = FIX / f"last-create.{scale}.json"
    out.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"wrote {out.relative_to(ROOT)}")
    return 0


def cmd_verify(base: str, scale: str) -> int:
    if not ensure_base(base):
        print(f"ERROR: {base} 不可达", file=sys.stderr)
        return 2
    exp = load_json(f"expected.{scale}.json")
    users = load_json("users.json")["users"]
    failures = []

    if len(users) != exp["users"]:
        failures.append(f"users.json count {len(users)} != {exp['users']}")

    # 会话壳数量（标题前缀）
    st, convs = http("GET", f"{base}/api/conversations")
    conv_list = convs if isinstance(convs, list) else []
    if st != 200:
        failures.append(f"list conversations HTTP {st}")
    else:
        shells = [c for c in conv_list if str(c.get("title", "")).startswith("[local-demo ")]
        need = exp["history_conversations_per_user"] * exp["users"]
        # live 标题也算 local-demo
        if len(shells) < need:
            failures.append(f"history-like convs {len(shells)} < {need}")
        else:
            print(f"OK history-like convs={len(shells)} (need>={need})")

    for u in users:
        st, body = http("GET", f"{base}/api/desktop/v1/memories", headers=hdr(u["principal"]))
        items = (body or {}).get("items") or []
        if st != 200:
            failures.append(f"{u['principal']} memories HTTP {st}")
            continue
        if len(items) < exp["memories_per_user_min"]:
            failures.append(
                f"{u['principal']} memories {len(items)} < {exp['memories_per_user_min']}"
            )
        titles = {i.get("title") for i in items}
        for t in exp.get("injection_memory_titles") or []:
            if t not in titles:
                failures.append(f"{u['principal']} missing injection title {t}")
        print(f"OK {u['principal']} memories={len(items)}")

    st, _ = http("POST", f"{base}/api/world/mock/ops/ensure-seed", {})
    # ensure-seed 幂等；允许 200
    if st != exp.get("industrial_ensure_seed_http", 200):
        failures.append(f"industrial ensure-seed HTTP {st}")
    else:
        print(f"OK industrial ensure-seed HTTP {st}")

    st, tasks = http("GET", f"{base}/api/tasks")
    items = tasks if isinstance(tasks, list) else (tasks or {}).get("items") or []
    if len(items) < exp.get("snapshot_tasks_min", 1):
        failures.append(f"tasks {len(items)} < min")
    else:
        print(f"OK tasks_visible={len(items)}")

    for g in exp.get("gaps") or []:
        print(f"GAP: {g}")

    if failures:
        print("VERIFY FAIL:")
        for f in failures:
            print(" -", f)
        return 1
    print("VERIFY PASS")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description="本地演示厂数据集 seed")
    p.add_argument("command", choices=["create", "reset", "verify"])
    p.add_argument("--base", default="http://127.0.0.1:8787")
    p.add_argument("--scale", default="small", choices=["small", "large"])
    p.add_argument("--seed", type=int, default=42425001)
    p.add_argument(
        "--live-turns",
        type=int,
        default=1,
        help="每用户 live 请求条数（走真实模型；0 或 --skip-live 跳过）",
    )
    p.add_argument(
        "--skip-live",
        action="store_true",
        help="不发 live 请求（只建会话壳与记忆；适合无模型或快速重建）",
    )
    args = p.parse_args()
    if args.command in ("create", "reset"):
        return cmd_create(
            args.base, args.scale, args.seed, args.live_turns, args.skip_live
        )
    return cmd_verify(args.base, args.scale)


if __name__ == "__main__":
    raise SystemExit(main())
