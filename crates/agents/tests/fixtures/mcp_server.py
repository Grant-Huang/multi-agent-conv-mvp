"""Deterministic MCP stdio fixture; never accesses network or user files."""
import json
import sys
import time
import os

calls = 0

for line in sys.stdin:
    req = json.loads(line)
    if "id" not in req:
        continue
    method = req["method"]
    params = req.get("params", {})
    if method == "initialize":
        result = {"protocolVersion": params["protocolVersion"],
                  "capabilities": {"tools": {}},
                  "serverInfo": {"name": "fixture", "version": "1"}}
    elif method == "tools/list":
        names = ["echo", "fail"] if not params.get("cursor") else ["slow", "resource", "bad_output"]
        result = {"tools": [{"name": name, "description": "fixture tool",
                             "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}},
                                             "required": ["text"], "additionalProperties": False},
                             **({"outputSchema": {"type": "object", "required": ["ok"]}}
                                if name == "bad_output" else {})} for name in names]}
        if not params.get("cursor"):
            result["nextCursor"] = "page2"
    elif method == "tools/call":
        calls += 1
        name = params["name"]
        if name == "slow":
            time.sleep(3)
        if name == "resource":
            result = {"content": [{"type": "resource", "resource": {
                "uri": "memory://report.md", "mimeType": "text/markdown", "text": "# Report"}}]}
        else:
            result = {"content": [{"type": "text", "text": params["arguments"]["text"]}],
                      "isError": name == "fail", "structuredContent": {"calls": calls, "pid": os.getpid()}}
            if name == "bad_output":
                result["structuredContent"] = {"wrong": True}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}), flush=True)
