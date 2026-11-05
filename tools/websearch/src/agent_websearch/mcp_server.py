"""MCP server（stdio）。需要安装可选依赖：agent-websearch[mcp]。

工具返回统一的 JSON 字符串；失败也以 status=error 返回而不是抛协议错误，模型能直接读到原因。
stdout 留给 MCP 协议，日志只写 stderr。
"""
import functools
import json
import sys

from . import api, operations
from .settings import Settings, configure_logging

INSTALL_HINT = "未安装 MCP 支持，请运行：uv tool install 'agent-websearch[mcp]'（或 pip install）"


def build_server():
    """创建 MCP server；没有安装 mcp 时抛 ImportError。"""
    import anyio.to_thread
    from mcp.server.mcpserver import MCPServer

    server = MCPServer("agent-websearch")

    async def call(operation: str, **params) -> str:
        envelope = await anyio.to_thread.run_sync(
            functools.partial(operations.run, operation, **params)
        )
        return json.dumps(envelope, ensure_ascii=False)

    @server.tool(structured_output=False, description=(
        "Search the web and return the extracted text of the top results in ONE call. "
        "This is the preferred tool for any question that needs up-to-date or external "
        "information. Failed pages are listed under `failures`; the search `results` "
        "(title, url, snippet) are always included."
    ))
    async def web_search_and_fetch(
        query: str,
        max_results: int = api.DEFAULT_COMBINED_RESULTS,
        max_chars_per_page: int = api.DEFAULT_COMBINED_CHARS,
    ) -> str:
        return await call("search_and_fetch", query=query, max_results=max_results,
                          max_chars_per_page=max_chars_per_page)

    @server.tool(structured_output=False, description=(
        "Search the web and return only titles, URLs and snippets (no page text). Use it to "
        "survey sources first; to read content prefer web_search_and_fetch."
    ))
    async def web_search(query: str, max_results: int = api.DEFAULT_SEARCH_RESULTS) -> str:
        return await call("search", query=query, max_results=max_results)

    @server.tool(structured_output=False, description=(
        "Fetch one web page and return its main text as Markdown. Use it to read a specific "
        "URL, or to get more of a page that was truncated."
    ))
    async def web_fetch(url: str, max_chars: int = api.DEFAULT_FETCH_CHARS) -> str:
        return await call("fetch", url=url, max_chars=max_chars)

    return server


def main() -> int:
    configure_logging(Settings.from_env().log_level)
    try:
        server = build_server()
    except ImportError:
        print(INSTALL_HINT, file=sys.stderr)
        return 1
    server.run(transport="stdio")
    return 0


if __name__ == "__main__":
    sys.exit(main())
