"""确定性的 dlp-egress 替身（D1 接口 1.0）：不联网、不读用户文件，只用于 AgentNexus 的外发守卫测试。

规则（对文本或 JSON 中的每个字符串生效）：
- 含 "SECRET-"        → BLOCK（不返回载荷）
- 含 "CONFIDENTIAL"   → REVIEW（返回预览，ready_to_send=false）
- "ACME"              → 替换为 "[CUSTOMER_001]"，ANONYMIZE
- 其他                → ALLOW
用法：python3 dlp_server.py <调用日志路径> [模式]
模式：normal（默认）/ bad_output（结构化返回不符合 schema）/ slow（每次调用睡 3 秒）/
      crash_after_first（第一次 evaluate 返回后进程退出）/ no_restore（restore 返回 restore_disabled）
"""
import json
import sys
import time

LOG = sys.argv[1]
MODE = sys.argv[2] if len(sys.argv) > 2 else "normal"
ERR = {"type": ["object", "null"], "properties": {"code": {"type": "string"}, "message": {"type": "string"}}}


def envelope(data_schema):
    return {"type": "object", "required": ["api_version", "status", "data", "error"],
            "properties": {"api_version": {"const": "1.0"}, "status": {"enum": ["success", "error"]},
                           "data": {"anyOf": [data_schema, {"type": "null"}]}, "error": ERR}}


EVAL_DATA = {"type": "object", "required": ["decision", "request_id", "ready_to_send", "reason_codes", "tokens"],
             "properties": {"decision": {"enum": ["ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"]},
                            "ready_to_send": {"type": "boolean"}}}
RESTORE_DATA = {"type": "object", "required": ["text", "restored"]}
TOOLS = [
    {"name": "egress_evaluate", "inputSchema": {"type": "object"}, "outputSchema": envelope(EVAL_DATA)},
    {"name": "egress_restore", "inputSchema": {"type": "object"}, "outputSchema": envelope(RESTORE_DATA)},
    {"name": "egress_revoke", "inputSchema": {"type": "object"},
     "outputSchema": envelope({"type": "object", "required": ["revoked"]})},
]


def walk(v, flags):
    if isinstance(v, str):
        if "SECRET-" in v:
            flags.add("BLOCK")
        if "CONFIDENTIAL" in v:
            flags.add("REVIEW")
        if "ACME" in v:
            flags.add("ANONYMIZE")
        return v.replace("ACME", "[CUSTOMER_001]")
    if isinstance(v, list):
        return [walk(x, flags) for x in v]
    if isinstance(v, dict):
        return {k: walk(x, flags) for k, x in v.items()}
    return v


def evaluate(args, n):
    flags = set()
    payload = args.get("text") if args.get("text") is not None else args.get("json_body")
    out = walk(payload, flags)
    decision = next((d for d in ("BLOCK", "REVIEW", "ANONYMIZE") if d in flags), "ALLOW")
    blocked = decision == "BLOCK"
    data = {"decision": decision, "policy_version": "egress-v1", "request_id": f"req_{n:04d}",
            "sanitized_text": None if blocked or args.get("text") is None else out,
            "sanitized_json": None if blocked or args.get("json_body") is None else out,
            "entities": [], "reason_codes": [decision.lower()], "validation": {},
            "tokens": ["[CUSTOMER_001]"] if "ANONYMIZE" in flags and not blocked else [],
            "ready_to_send": decision in ("ALLOW", "ANONYMIZE"), "elapsed_ms": 0.1}
    return {"api_version": "1.0", "status": "success", "data": data, "error": None}


n = 0
for line in sys.stdin:
    req = json.loads(line)
    if "id" not in req:
        continue
    method, params = req["method"], req.get("params", {})
    exit_after = False
    if method == "initialize":
        result = {"protocolVersion": params["protocolVersion"], "capabilities": {"tools": {}},
                  "serverInfo": {"name": "dlp-fixture", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [dict(t, description="fixture") for t in TOOLS]}
    elif method == "tools/call":
        n += 1
        name, args = params["name"], params.get("arguments", {})
        with open(LOG, "a", encoding="utf-8") as f:
            f.write(json.dumps({"tool": name, "args": args}, ensure_ascii=False) + "\n")
        if MODE == "slow":
            time.sleep(3)
        if name == "egress_evaluate":
            sc = evaluate(args, n)
            exit_after = MODE == "crash_after_first"
        elif name == "egress_restore":
            if MODE == "no_restore":
                sc = {"api_version": "1.0", "status": "error", "data": None,
                      "error": {"code": "restore_disabled", "message": "off"}}
            else:
                text = args["text"].replace("[CUSTOMER_001]", "ACME")
                sc = {"api_version": "1.0", "status": "success", "error": None,
                      "data": {"text": text, "restored": args["text"].count("[CUSTOMER_001]"),
                               "unknown_tokens": [], "refused_reason": None}}
        else:
            sc = {"api_version": "1.0", "status": "success", "data": {"revoked": 0}, "error": None}
        if MODE == "bad_output":
            sc = {"unexpected": True}
        result = {"content": [{"type": "text", "text": json.dumps(sc)}], "structuredContent": sc, "isError": False}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}), flush=True)
    if exit_after:
        sys.exit(0)
