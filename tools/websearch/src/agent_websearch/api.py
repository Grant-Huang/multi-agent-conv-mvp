"""对外的核心函数：参数校验 + 调度。CLI、MCP server、Python 调用都走这里。"""
import logging
import time
from concurrent.futures import ThreadPoolExecutor
from urllib.parse import urlsplit

from . import providers
from .errors import BlockedUrl, FetchFailed, InvalidArgument, WebSearchError
from .fetch_jina import fetch_jina
from .fetch_local import fetch_local
from .models import FetchFailure, Page, SearchAndFetchResult, SearchResult
from .settings import Settings
from .url_guard import check_url

logger = logging.getLogger("agent_websearch")

MIN_CHARS_LIMIT = 500
MAX_CHARS_LIMIT = 100000
DEFAULT_FETCH_CHARS = 8000
DEFAULT_SEARCH_RESULTS = 8
MAX_SEARCH_RESULTS = 20
MAX_QUERY_LENGTH = 500
DEFAULT_COMBINED_RESULTS = 3
MAX_COMBINED_RESULTS = 8
DEFAULT_COMBINED_CHARS = 3000
MAX_FETCH_WORKERS = 4


def search(query: str, max_results: int = DEFAULT_SEARCH_RESULTS,
           provider: str | None = None, settings: Settings | None = None) -> list[SearchResult]:
    """只搜索，返回标题、URL、摘要。"""
    settings = settings or Settings.from_env()
    query = _check_query(query)
    _check_int("max_results", max_results, 1, MAX_SEARCH_RESULTS)
    source = providers.get_provider(provider or settings.provider)
    started = time.monotonic()
    results = source.search(query, max_results, settings.search_timeout)
    logger.info("搜索完成 provider=%s results=%d %dms", source.name, len(results),
                int((time.monotonic() - started) * 1000))
    return results


def search_and_fetch(query: str, max_results: int = DEFAULT_COMBINED_RESULTS,
                     max_chars_per_page: int = DEFAULT_COMBINED_CHARS,
                     provider: str | None = None,
                     settings: Settings | None = None) -> SearchAndFetchResult:
    """一次调用：搜索后并发抓取前几条结果的正文。单页失败不影响整体，列在 failures 里。"""
    settings = settings or Settings.from_env()
    _check_int("max_results", max_results, 1, MAX_COMBINED_RESULTS)
    _check_max_chars(max_chars_per_page)
    source_name = provider or settings.provider
    results = search(query, max_results, source_name, settings)
    pages: list[Page] = []
    failures: list[FetchFailure] = []
    if results:
        with ThreadPoolExecutor(max_workers=min(len(results), MAX_FETCH_WORKERS)) as pool:
            futures = [
                (r.url, pool.submit(fetch, r.url, max_chars_per_page, settings)) for r in results
            ]
            for url, future in futures:
                try:
                    pages.append(future.result())
                except WebSearchError as exc:
                    failures.append(FetchFailure(url, exc.code, exc.message))
                except Exception as exc:  # 单页的意外错误不能拖垮整批，细节不外露
                    logger.exception("抓取出现未预期的错误 host=%s", urlsplit(url).hostname)
                    failures.append(FetchFailure(url, WebSearchError.code, type(exc).__name__))
    return SearchAndFetchResult(
        query=query.strip(), provider=source_name,
        results=results, pages=pages, failures=failures,
    )


def fetch(url: str, max_chars: int = DEFAULT_FETCH_CHARS,
          settings: Settings | None = None) -> Page:
    """抓取网页正文（Markdown）。按 settings.fetch_order 依次尝试本机 / Jina。"""
    settings = settings or Settings.from_env()
    _check_max_chars(max_chars)
    check_url(url, settings.allow_private)
    url = url.strip()
    started = time.monotonic()
    failures = []
    for method in settings.fetch_methods:
        fetcher = fetch_local if method == "local" else fetch_jina
        try:
            fetched = fetcher(url, settings)
        except (BlockedUrl, InvalidArgument):
            raise
        except WebSearchError as exc:
            failures.append(f"{method}: {exc.message}")
            continue
        content, truncated = _truncate(fetched.content, max_chars)
        elapsed_ms = int((time.monotonic() - started) * 1000)
        logger.info("抓取成功 host=%s method=%s chars=%d %dms",
                    urlsplit(url).hostname, method, len(fetched.content), elapsed_ms)
        return Page(
            url=url,
            final_url=fetched.final_url,
            title=fetched.title,
            content=content,
            method=method,
            chars=len(fetched.content),
            truncated=truncated,
            elapsed_ms=elapsed_ms,
        )
    raise FetchFailed("；".join(failures))


def _check_query(query: str) -> str:
    if not isinstance(query, str) or not query.strip():
        raise InvalidArgument("query 不能为空")
    query = query.strip()
    if len(query) > MAX_QUERY_LENGTH:
        raise InvalidArgument(f"query 不能超过 {MAX_QUERY_LENGTH} 个字符")
    return query


def _check_int(name: str, value: int, low: int, high: int) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or not low <= value <= high:
        raise InvalidArgument(f"{name} 必须是 {low} 到 {high} 之间的整数")


def _check_max_chars(max_chars: int) -> None:
    _check_int("max_chars", max_chars, MIN_CHARS_LIMIT, MAX_CHARS_LIMIT)


def _truncate(content: str, max_chars: int) -> tuple[str, bool]:
    """超长时尽量在段落边界截断（保留至少一半长度），否则硬截。"""
    if len(content) <= max_chars:
        return content, False
    head = content[:max_chars]
    for separator in ("\n\n", "\n"):
        cut = head.rfind(separator)
        if cut >= max_chars // 2:
            return head[:cut].rstrip(), True
    return head, True
