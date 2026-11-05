"""CLI 与 MCP server 共用：调用 api 并包装成统一的 JSON 结构。

成功：{"status": "success", "data": {...}, "message": ""}
失败：{"status": "error", "data": {"code": "..."}, "message": "..."}
"""
import logging

from . import api
from .errors import WebSearchError
from .settings import Settings

logger = logging.getLogger("agent_websearch")


def run(operation: str, **params) -> dict:
    try:
        handler = _HANDLERS.get(operation)
        if handler is None:
            raise ValueError(f"未知操作：{operation}")
        data, message = handler(Settings.from_env(), **params)
    except WebSearchError as exc:
        return _envelope("error", {"code": exc.code}, exc.message)
    except Exception as exc:  # 意外错误只对外暴露异常类型，细节写日志
        logger.exception("操作 %s 出现未预期的错误", operation)
        return _envelope(
            "error", {"code": WebSearchError.code}, f"内部错误：{type(exc).__name__}"
        )
    return _envelope("success", data, message)


def _envelope(status: str, data: dict, message: str) -> dict:
    return {"status": status, "data": data, "message": message}


def _search_and_fetch(settings, query, max_results, max_chars_per_page, provider=None):
    result = api.search_and_fetch(
        query, max_results=max_results, max_chars_per_page=max_chars_per_page,
        provider=provider, settings=settings,
    )
    message = f"{len(result.failures)} 个页面抓取失败" if result.failures else ""
    return result.to_dict(), message


def _search(settings, query, max_results, provider=None):
    results = api.search(query, max_results=max_results, provider=provider, settings=settings)
    name = results[0].provider if results else (provider or settings.provider)
    data = {"query": query, "provider": name, "results": [r.to_dict() for r in results]}
    return data, ""


def _fetch(settings, url, max_chars):
    return api.fetch(url, max_chars=max_chars, settings=settings).to_dict(), ""


_HANDLERS = {
    "search_and_fetch": _search_and_fetch,
    "search": _search,
    "fetch": _fetch,
}
