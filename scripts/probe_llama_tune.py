#!/usr/bin/env python3
"""量一次 llama-server 当前进程的首 token、生成速度和内存。

不改服务参数。对比 -b/-ub、--cache-reuse、思考开关时，先按 scripts/llama_server_35b.sh
重启，再跑本脚本，一次只变一项。流式请求，读完即停，不留着生成。
"""

import json
import os
import subprocess
import time
import urllib.request

HOST = os.environ.get("LLAMA_HOST", "127.0.0.1")
PORT = int(os.environ.get("LLAMA_PORT", "8200"))
BASE = f"http://{HOST}:{PORT}"

# 大约两千字的固定前缀，用来区分「整段都要重算」和「只改了开头 / 只改了用户消息」。
BODY = "规则：闲聊、解释概念、简单计算直接回答；写文件、改代码、查资料交给后台。" * 40


def system_prompt(head):
    return head + BODY

SCHEMA = {
    "type": "json_schema",
    "json_schema": {
        "name": "turn",
        "strict": True,
        "schema": {
            "type": "object",
            "properties": {"action": {"type": "string", "enum": ["respond", "delegate"]}},
            "required": ["action"],
            "additionalProperties": False,
        },
    },
}


def rss_mib():
    out = subprocess.check_output(["lsof", "-nP", f"-iTCP:{PORT}", "-sTCP:LISTEN", "-t"], text=True)
    pid = out.strip().splitlines()[0]
    rss = subprocess.check_output(["ps", "-o", "rss=", "-p", pid], text=True)
    return int(rss.strip()) // 1024, int(pid)


def post(path, payload, timeout):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(
        BASE + path, data=data, headers={"Content-Type": "application/json"}
    )
    return urllib.request.urlopen(req, timeout=timeout)


def thinking_extra():
    if os.environ.get("PROBE_THINKING") == "off":
        return {"chat_template_kwargs": {"enable_thinking": False}}
    return {}


def stream_chat(messages, max_tokens, extra=None):
    body = {
        "model": "probe",
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0,
        "stream": True,
        "stream_options": {"include_usage": True},
        "id_slot": 0,
    }
    body.update(thinking_extra())
    if extra:
        body.update(extra)
    t0 = time.perf_counter()
    ttft = None
    usage = {}
    content = []
    reasoning = []
    with post("/v1/chat/completions", body, timeout=180) as resp:
        for raw in resp:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            data = line[5:].strip()
            if data == "[DONE]":
                break
            try:
                ev = json.loads(data)
            except json.JSONDecodeError:
                continue
            if ev.get("usage"):
                usage = ev["usage"]
            choices = ev.get("choices") or []
            if not choices:
                continue
            delta = choices[0].get("delta") or {}
            piece = delta.get("content") or ""
            thought = delta.get("reasoning_content") or ""
            if (piece or thought) and ttft is None:
                ttft = time.perf_counter() - t0
            if piece:
                content.append(piece)
            if thought:
                reasoning.append(thought)
    total = time.perf_counter() - t0
    completion = usage.get("completion_tokens")
    gen = None
    if completion and ttft is not None and total > ttft:
        gen = completion / (total - ttft)
    return {
        "ttft_ms": None if ttft is None else round(ttft * 1000),
        "total_ms": round(total * 1000),
        "completion_tokens": completion,
        "gen_tok_s": None if gen is None else round(gen, 1),
        "reasoning_chars": sum(len(s) for s in reasoning),
        "content_chars": sum(len(s) for s in content),
    }


def main():
    label = os.environ.get("PROBE_LABEL", "baseline")
    mem, pid = rss_mib()
    # 无关请求，把模型唤醒，避免把加载算进第一条
    stream_chat([{"role": "user", "content": "回复一个字：好"}], 8)
    head = os.environ.get("PROBE_HEAD", "编号 cold。")
    cold_msgs = [
        {"role": "system", "content": system_prompt(head)},
        {"role": "user", "content": "帮我写一份本周工作周报，存成 markdown。"},
    ]
    cold = stream_chat(cold_msgs, 256, {"response_format": SCHEMA})
    # 系统提示词相同、用户消息不同：普通提示词缓存能复用前缀
    prefix_msgs = [
        {"role": "system", "content": system_prompt(head)},
        {"role": "user", "content": "今天上海天气怎么样？"},
    ]
    prefix = stream_chat(prefix_msgs, 256, {"response_format": SCHEMA})
    # 只改了开头一行，后面的长正文相同：要靠 KV 移位（--cache-reuse）才可能复用
    shift_msgs = [
        {"role": "system", "content": system_prompt("编号 shift-" + head)},
        {"role": "user", "content": "帮我写一份本周工作周报，存成 markdown。"},
    ]
    shift = stream_chat(shift_msgs, 256, {"response_format": SCHEMA})
    decode = stream_chat(
        [{"role": "user", "content": "从 1 数到 40，数字之间用空格，不要其他文字。"}],
        80,
    )
    mem2, _ = rss_mib()
    report = {
        "label": label,
        "pid": pid,
        "rss_mib": max(mem, mem2),
        "thinking": os.environ.get("PROBE_THINKING", "default"),
        "cold_ttft_ms": cold["ttft_ms"],
        "cold_total_ms": cold["total_ms"],
        "cold_completion_tokens": cold["completion_tokens"],
        "cold_gen_tok_s": cold["gen_tok_s"],
        "cold_reasoning_chars": cold["reasoning_chars"],
        "prefix_ttft_ms": prefix["ttft_ms"],
        "shift_ttft_ms": shift["ttft_ms"],
        "decode_ttft_ms": decode["ttft_ms"],
        "decode_gen_tok_s": decode["gen_tok_s"],
        "decode_completion_tokens": decode["completion_tokens"],
        "runs": {"cold": cold, "prefix": prefix, "shift": shift, "decode": decode},
    }
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
