"""模拟的 DLP 服务（MCP stdio），按 docs/dlp/CONTRACT.md 返回结构化结果。只用于测试，不联网、不读用户文件。

行为（由环境变量控制）：
- DLP_MODE：
  - anonymize（默认）：把文本里的 "ACME" 换成 [CUSTOMER_001]（同一任务内稳定）；没有就 ALLOW
  - review：一律 REVIEW
  - bad_output：返回不符合 output schema 的结构
  - bad_version：api_version 为 "2"
  - slow：每次调用先睡 30 秒（用来测超时）
- 文本里含 "sk-live-" 时一律 BLOCK（secret_blocked），不论模式
- DLP_LOG：每次调用追加一行 JSON（工具名、task_id、channel、destination_id、purpose、context），不记文本内容
"""
import json
import os
import sys
import time

MODE = os.environ.get("DLP_MODE", "anonymize")
LOG = os.environ.get("DLP_LOG")
vault = {}  # task_id -> {token: 原文}

ENVELOPE = {
    "type": "object",
    "required": ["status", "api_version", "data", "error"],
    "properties": {
        "status": {"enum": ["success", "error"]},
        "api_version": {"type": "string"},
    },
}
TOOLS = [
    {
        "name": "egress_evaluate",
        "description": "fixture",
        "inputSchema": {
            "type": "object",
            "properties": {k: {} for k in
                           ["task_id", "channel", "destination_id", "purpose", "text", "json_body", "constraints"]},
            "required": ["task_id", "channel", "destination_id", "purpose"],
            "additionalProperties": False,
        },
        "outputSchema": ENVELOPE,
    },
    {
        "name": "egress_restore",
        "description": "fixture",
        "inputSchema": {"type": "object",
                        "properties": {"task_id": {}, "text": {}, "context": {}},
                        "required": ["task_id", "text"], "additionalProperties": False},
        "outputSchema": ENVELOPE,
    },
    {
        "name": "egress_revoke",
        "description": "fixture",
        "inputSchema": {"type": "object", "properties": {"task_id": {}},
                        "required": ["task_id"], "additionalProperties": False},
        "outputSchema": ENVELOPE,
    },
]


def log(tool, args):
    if LOG:
        rec = {"tool": tool}
        for k in ["task_id", "channel", "destination_id", "purpose", "context"]:
            if k in args:
                rec[k] = args[k]
        with open(LOG, "a") as f:
            f.write(json.dumps(rec) + "\n")


def ok(data):
    return {"status": "success", "api_version": "2" if MODE == "bad_version" else "1", "data": data, "error": None}


def evaluate(a):
    text = a.get("text") or ""
    base = {"policy_version": "fixture-v1", "request_id": "req_fixture", "entities": [], "tokens": [],
            "sanitized_json": None, "validation": {}, "elapsed_ms": 0.1}
    if "sk-live-" in text:
        return ok({**base, "decision": "BLOCK", "ready_to_send": False, "sanitized_text": None,
                   "reason_codes": ["secret_blocked"],
                   "entities": [{"type": "SECRET", "start": 0, "end": 1, "score": 1.0, "source": "rule"}]})
    if MODE == "review":
        return ok({**base, "decision": "REVIEW", "ready_to_send": False,
                   "sanitized_text": "[REDACTED_CONFIDENTIAL]", "reason_codes": ["confidential_requires_review"]})
    if "ACME" in text:
        vault.setdefault(a["task_id"], {})["[CUSTOMER_001]"] = "ACME"
        return ok({**base, "decision": "ANONYMIZE", "ready_to_send": True,
                   "sanitized_text": text.replace("ACME", "[CUSTOMER_001]"),
                   "reason_codes": ["customer_tokenized"], "tokens": ["[CUSTOMER_001]"],
                   "entities": [{"type": "CUSTOMER", "start": 0, "end": 4, "score": 0.99, "source": "master_data"}]})
    return ok({**base, "decision": "ALLOW", "ready_to_send": True, "sanitized_text": text, "reason_codes": []})


def restore(a):
    if a.get("context", "task_postprocess") != "local_ui":
        return ok({"text": a["text"], "restored": 0, "unknown_tokens": [], "refused_reason": "context_not_allowed"})
    text, n = a["text"], 0
    for token, value in vault.get(a["task_id"], {}).items():
        if token in text:
            text, n = text.replace(token, value), n + 1
    return ok({"text": text, "restored": n, "unknown_tokens": [], "refused_reason": None})


def revoke(a):
    n = len(vault.pop(a["task_id"], {}))
    return ok({"revoked": n})


for line in sys.stdin:
    req = json.loads(line)
    if "id" not in req:
        continue
    method, params = req["method"], req.get("params", {})
    if method == "initialize":
        result = {"protocolVersion": params["protocolVersion"], "capabilities": {"tools": {}},
                  "serverInfo": {"name": "dlp-fixture", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        name, args = params["name"], params.get("arguments", {})
        log(name, args)
        if MODE == "slow":
            time.sleep(30)
        data = {"egress_evaluate": evaluate, "egress_restore": restore, "egress_revoke": revoke}[name](args)
        if MODE == "bad_output":
            data = {"unexpected": True}
        result = {"content": [{"type": "text", "text": json.dumps(data, ensure_ascii=False)}],
                  "structuredContent": data, "isError": False}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}, ensure_ascii=False), flush=True)
