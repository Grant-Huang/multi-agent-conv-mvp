"""M3.3：同一批查询跑 ddg / exa / parallel，记下成功、正文和延迟。

用法：在仓库根目录执行
    cd tools/websearch && uv run python ../../scripts/ab_m3_3.py
结果写到 .tmp/ab_m3_3.json。不调用本地模型。
"""
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "websearch" / "src"))

from agent_websearch import api  # noqa: E402
from agent_websearch.errors import WebSearchError  # noqa: E402

QUERIES = [
    {"id": "zh-news", "lang": "zh", "query": "2026年11月5日 AI agent 架构"},
    {"id": "zh-policy", "lang": "zh", "query": "2026 上海 人工智能 产业政策"},
    {"id": "zh-local", "lang": "zh", "query": "Qwen 本地模型 工具调用 结构化输出"},
    {"id": "en-llama", "lang": "en", "query": "llama.cpp server slot scheduling KV cache"},
    {"id": "en-orch", "lang": "en", "query": "agents as tools versus handoff orchestration"},
    {"id": "en-acp", "lang": "en", "query": "Agent Client Protocol v2 session prompt migration"},
]
PROVIDERS = ("ddg", "exa", "parallel")


def percentile(values, p):
    if not values:
        return None
    ordered = sorted(values)
    index = round((len(ordered) - 1) * p)
    return ordered[index]


def cjk_chars(text):
    return sum(1 for ch in text if "\u4e00" <= ch <= "\u9fff")


def main():
    rows = []
    for provider in PROVIDERS:
        for item in QUERIES:
            started = time.perf_counter()
            try:
                out = api.search_and_fetch(
                    item["query"], max_results=3, max_chars_per_page=1500, provider=provider,
                )
                elapsed = round((time.perf_counter() - started) * 1000)
                pages = []
                for page in out.pages:
                    pages.append({
                        "url": page.url,
                        "title": page.title[:120],
                        "method": page.method,
                        "chars": page.chars,
                        "cjk": cjk_chars(page.content),
                        "content": page.content[:240],
                    })
                rows.append({
                    "provider": provider,
                    "id": item["id"],
                    "lang": item["lang"],
                    "query": item["query"],
                    "ms": elapsed,
                    "error": None,
                    "n_results": len(out.results),
                    "n_pages": len(out.pages),
                    "n_failures": len(out.failures),
                    "chars": sum(page.chars for page in out.pages),
                    "results": [
                        {"title": r.title[:120], "url": r.url, "snippet": r.snippet[:180]}
                        for r in out.results
                    ],
                    "pages": pages,
                    "failures": [
                        {"url": f.url, "code": f.code, "message": f.message} for f in out.failures
                    ],
                })
            except WebSearchError as exc:
                elapsed = round((time.perf_counter() - started) * 1000)
                rows.append({
                    "provider": provider, "id": item["id"], "lang": item["lang"],
                    "query": item["query"], "ms": elapsed, "error": f"{exc.code}: {exc.message}",
                    "n_results": 0, "n_pages": 0, "n_failures": 0, "chars": 0,
                    "results": [], "pages": [], "failures": [],
                })
            row = rows[-1]
            print(
                f"{provider:8} {row['id']:10} {row['ms']:6}ms "
                f"results={row['n_results']} pages={row['n_pages']} chars={row['chars']} "
                f"err={row['error']}",
                flush=True,
            )
    summary = {}
    for provider in PROVIDERS:
        mine = [row for row in rows if row["provider"] == provider]
        ok = [row for row in mine if row["error"] is None and row["n_results"] > 0]
        lat = [row["ms"] for row in mine]
        zh = [row for row in mine if row["lang"] == "zh"]
        zh_pages = [page for row in zh for page in row["pages"]]
        zh_ok = [page for page in zh_pages if page["cjk"] >= 30]
        summary[provider] = {
            "queries": len(mine),
            "with_results": len(ok),
            "errors": sum(1 for row in mine if row["error"]),
            "mean_results": round(sum(row["n_results"] for row in mine) / len(mine), 2),
            "total_chars": sum(row["chars"] for row in mine),
            "p50_ms": percentile(lat, 0.5),
            "p95_ms": percentile(lat, 0.95),
            "zh_pages": len(zh_pages),
            "zh_pages_with_cjk": len(zh_ok),
        }
    out_path = Path(__file__).resolve().parents[1] / ".tmp" / "ab_m3_3.json"
    out_path.parent.mkdir(exist_ok=True)
    out_path.write_text(json.dumps({"summary": summary, "rows": rows}, ensure_ascii=False, indent=2))
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    print("wrote", out_path)


if __name__ == "__main__":
    main()
