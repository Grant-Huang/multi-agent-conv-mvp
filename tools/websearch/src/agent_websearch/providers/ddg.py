"""DuckDuckGo 搜索（父进程侧）。

ddgs 底层的 primp 可能在持有 GIL 时挂住，线程超时杀不掉，所以每次搜索都放进子进程，
由 `subprocess.run(timeout=...)` 硬性杀掉。查询词通过 stdin 传入，不出现在命令行里。
"""
import json
import logging
import subprocess
import sys

from ..errors import ProviderError, Timeout
from ..models import SearchResult

logger = logging.getLogger("agent_websearch")

WORKER_COMMAND = [sys.executable, "-m", "agent_websearch.providers._ddg_worker"]


class DdgProvider:
    name = "ddg"

    def search(self, query: str, max_results: int, timeout: float) -> list[SearchResult]:
        request = json.dumps({"query": query, "max_results": max_results})
        try:
            proc = subprocess.run(
                WORKER_COMMAND, input=request, capture_output=True,
                text=True, encoding="utf-8", timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            raise Timeout(f"DuckDuckGo 搜索超时（{timeout:g}s）") from None
        payload = _load(proc.stdout)
        if proc.returncode != 0:
            if isinstance(payload, dict) and payload.get("error"):
                raise ProviderError(str(payload["error"]))
            raise ProviderError(f"搜索子进程异常退出（退出码 {proc.returncode}）")
        if not isinstance(payload, list):
            raise ProviderError("搜索子进程返回了无法解析的内容")
        return _to_results(payload, max_results)


def _load(text: str):
    try:
        return json.loads(text)
    except ValueError:
        return None


def _to_results(items: list, max_results: int) -> list[SearchResult]:
    results: list[SearchResult] = []
    seen: set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            continue
        url = str(item.get("href") or "").strip()
        if not url.lower().startswith(("http://", "https://")) or url in seen:
            continue
        seen.add(url)
        results.append(SearchResult(
            title=str(item.get("title") or "").strip(),
            url=url,
            snippet=str(item.get("body") or "").strip(),
            provider=DdgProvider.name,
            rank=len(results) + 1,
        ))
        if len(results) >= max_results:
            break
    return results
