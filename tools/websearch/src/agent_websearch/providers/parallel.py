"""Parallel 托管搜索（https://search.parallel.ai/mcp），不需要 API key。

只取标题、URL 和摘录。正文仍走现有抓取管线。
"""
import json

import httpx

from ..errors import ProviderError, Timeout
from ..models import SearchResult

PARALLEL_MCP_URL = "https://search.parallel.ai/mcp"
_HEADERS = {"Accept": "application/json, text/event-stream", "Content-Type": "application/json"}
_SNIPPET_LIMIT = 500


class ParallelProvider:
    name = "parallel"

    def search(self, query: str, max_results: int, timeout: float) -> list[SearchResult]:
        body = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "web_search",
                "arguments": {"objective": query, "search_queries": [query]},
            },
        }
        try:
            response = httpx.post(PARALLEL_MCP_URL, json=body, headers=_HEADERS, timeout=timeout)
        except httpx.TimeoutException:
            raise Timeout(f"Parallel 搜索超时（{timeout:g}s）") from None
        except httpx.HTTPError as exc:
            raise ProviderError(f"Parallel 请求失败：{type(exc).__name__}") from None
        if response.status_code != 200:
            raise ProviderError(f"Parallel HTTP {response.status_code}")
        return _to_results(_inner_results(response.text), max_results)


def _inner_results(raw: str) -> list:
    text = _mcp_text(raw)
    try:
        obj = json.loads(text)
    except ValueError:
        raise ProviderError("Parallel 返回了无法解析的内容") from None
    results = obj.get("results") if isinstance(obj, dict) else None
    if not isinstance(results, list):
        raise ProviderError("Parallel 返回了无法解析的内容")
    return results


def _mcp_text(raw: str) -> str:
    blobs = []
    stripped = raw.strip()
    if stripped.startswith("{"):
        blobs.append(stripped)
    for line in raw.splitlines():
        if line.startswith("data:"):
            blobs.append(line[5:].strip())
    error = None
    texts = []
    for blob in blobs:
        try:
            obj = json.loads(blob)
        except ValueError:
            continue
        if "error" in obj and "result" not in obj:
            err = obj["error"]
            error = err.get("message") if isinstance(err, dict) else str(err)
            continue
        for block in (obj.get("result") or {}).get("content") or []:
            if isinstance(block, dict) and block.get("text"):
                texts.append(str(block["text"]))
    if texts:
        return "\n".join(texts)
    if error:
        raise ProviderError(str(error))
    raise ProviderError("Parallel 返回了无法解析的内容")


def _to_results(items: list, max_results: int) -> list[SearchResult]:
    results: list[SearchResult] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            continue
        url = str(item.get("url") or "").strip()
        if not url.lower().startswith(("http://", "https://")) or url in seen:
            continue
        seen.add(url)
        excerpts = item.get("excerpts") or []
        if isinstance(excerpts, str):
            excerpts = [excerpts]
        snippet = " ".join(" ".join(str(part).split()) for part in excerpts if str(part).strip())
        results.append(SearchResult(
            title=str(item.get("title") or "").strip(),
            url=url,
            snippet=snippet[:_SNIPPET_LIMIT],
            provider=ParallelProvider.name,
            rank=len(results) + 1,
        ))
        if len(results) >= max_results:
            break
    return results
