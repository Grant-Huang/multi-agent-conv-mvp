"""用 8200 上的 35B 给 M3.3 原始结果打分。固定槽位 5，不重启 llama-server。

读取 .tmp/ab_m3_3.json，写出 .tmp/ab_m3_3_judge.json。
"""
import json
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / ".tmp" / "ab_m3_3.json"
OUT = ROOT / ".tmp" / "ab_m3_3_judge.json"
URL = "http://127.0.0.1:8200/v1/chat/completions"
MODEL = "./qwen3.8-35b-a3b/Qwen3.8-35B-A3B-Q4_K_M.gguf"
SLOT = 5

RUBRIC = """你是搜索结果评委。只根据给定材料打分，不要补充材料里没有的事实。
每项 1 到 5 的整数：
- relevance：结果是否在回答这条查询
- accuracy：摘录之间是否自洽；空结果、抓取失败、只有导航时给低分
- coverage：是否覆盖查询里的主要方面，而不是同一页的重复链接
- structure：标题、摘要、正文是否能让人读懂
- citations：是否给出可核对的 http(s) 链接，且链接和标题对得上
只输出一个 JSON 对象，不要其他文字：
{"relevance":1,"accuracy":1,"coverage":1,"structure":1,"citations":1,"note":"一句中文理由"}"""


def pack(row):
    lines = [f"查询：{row['query']}", f"来源：{row['provider']}"]
    if row.get("error"):
        lines.append(f"错误：{row['error']}")
    if not row["results"]:
        lines.append("没有搜索结果。")
    for item in row["results"]:
        lines.append(f"- {item['title']}\n  {item['url']}\n  摘要：{item['snippet']}")
    for page in row["pages"]:
        lines.append(f"正文 {page['method']} {page['chars']}字 {page['url']}\n{page['content']}")
    for fail in row["failures"]:
        lines.append(f"抓取失败 {fail['url']} {fail['code']} {fail['message']}")
    return "\n".join(lines)


def judge(material):
    body = {
        "model": MODEL,
        "temperature": 0,
        "max_tokens": 500,
        "id_slot": SLOT,
        "chat_template_kwargs": {"enable_thinking": False},
        "messages": [
            {"role": "system", "content": RUBRIC},
            {"role": "user", "content": material},
        ],
    }
    req = urllib.request.Request(
        URL, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        payload = json.load(resp)
    text = payload["choices"][0]["message"].get("content") or ""
    start = text.find("{")
    end = text.rfind("}")
    if start < 0 or end < start:
        raise ValueError(text[:200])
    score = json.loads(text[start:end + 1])
    keys = ("relevance", "accuracy", "coverage", "structure", "citations")
    for key in keys:
        score[key] = int(score[key])
        if not 1 <= score[key] <= 5:
            raise ValueError(score)
    score["mean"] = round(sum(score[key] for key in keys) / 5, 2)
    return score


def main():
    rows = json.loads(SOURCE.read_text())["rows"]
    judged = []
    for row in rows:
        score = judge(pack(row))
        judged.append({
            "provider": row["provider"], "id": row["id"], "query": row["query"], **score,
        })
        print(f"{row['provider']:8} {row['id']:10} {score['mean']} {score['note']}", flush=True)
    summary = {}
    for provider in ("ddg", "exa", "parallel"):
        mine = [item for item in judged if item["provider"] == provider]
        summary[provider] = {
            key: round(sum(item[key] for item in mine) / len(mine), 2)
            for key in ("relevance", "accuracy", "coverage", "structure", "citations", "mean")
        }
    OUT.write_text(json.dumps({"summary": summary, "rows": judged}, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print("wrote", OUT)


if __name__ == "__main__":
    main()
