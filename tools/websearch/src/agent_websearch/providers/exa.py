"""Exa 托管搜索（https://mcp.exa.ai），不需要 API key。

只取标题、URL 和摘要。正文仍走现有抓取管线（本机优先，失败再用 Jina），
不把 Exa 返回的长文直接当成页面内容。
"""
import json
import re

import httpx

from ..errors import ProviderError, Timeout
from ..models import SearchResult

EXA_MCP_URL = "https://mcp.exa.ai/mcp"
_HEADERS = {"Accept": "application/json, text/event-stream", "Content-Type": "application/json"}
_ITEM = re.compile(
    r"Title:[ \t]*(.*?)\r?\nURL:[ \t]*(\S+)\r?\n.*?Highlights:\r?\n(.*?)(?=\r?\n---\r?\n|\Z)",
    re.S,
)
_SNIPPET_LIMIT = 500


class ExaProvider:
    name = "exa"

    def search(self, query: str, max_results: int, timeout: float) -> list[SearchResult]:
        body = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "web_search_exa",
                "arguments": {"query": query, "numResults": max_results, "objective": query},
            },
        }
        try:
            response = httpx.post(EXA_MCP_URL, json=body, headers=_HEADERS, timeout=timeout)
        except httpx.TimeoutException:
            raise Timeout(f"Exa 搜索超时（{timeout:g}s）") from None
        except httpx.HTTPError as exc:
            raise ProviderError(f"Exa 请求失败：{type(exc).__name__}") from None
        if response.status_code != 200:
            raise ProviderError(f"Exa HTTP {response.status_code}")
        text = _mcp_text(response.text)
        return _to_results(text, max_results)


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
        if "error" in obj:
            err = obj["error"]
            error = err.get("message") if isinstance(err, dict) else str(err)
            continue
        for block in (obj.get("result") or {}).get("content") or []:
            if isinstance(block, dict) and block.get("type") == "text" and block.get("text"):
                texts.append(str(block["text"]))
    if texts:
        return "\n".join(texts)
    if error:
        raise ProviderError(str(error))
    raise ProviderError("Exa 返回了无法解析的内容")


def _to_results(text: str, max_results: int) -> list[SearchResult]:
    results: list[SearchResult] = []
    seen: set[str] = set()
    for match in _ITEM.finditer(text):
        url = match.group(2).strip()
        if not url.lower().startswith(("http://", "https://")) or url in seen:
            continue
        seen.add(url)
        snippet = " ".join(match.group(3).split())
        results.append(SearchResult(
            title=match.group(1).strip(),
            url=url,
            snippet=snippet[:_SNIPPET_LIMIT],
            provider=ExaProvider.name,
            rank=len(results) + 1,
        ))
        if len(results) >= max_results:
            break
    return results
