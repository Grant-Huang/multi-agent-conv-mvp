"""DuckDuckGo 搜索（子进程侧）：从 stdin 读 JSON 请求，把结果以 JSON 写到 stdout。

成功：退出码 0，stdout 是结果列表；失败：退出码 1，stdout 是 {"error": "..."}。

ddgs 的 auto 后端对同一个查询时好时坏（实测会随机报 "No results found" 或被限流），
所以"无结果 / 限流 / 超时"最多重试 MAX_ATTEMPTS 次；其他错误不重试。
"""
import json
import sys
import time
from typing import TextIO

MAX_ATTEMPTS = 3
RETRY_DELAY = 1.0
_TRANSIENT_ERRORS = ("RatelimitException", "TimeoutException")


def _fail(stdout: TextIO, message: str) -> int:
    json.dump({"error": message}, stdout)
    return 1


def _parse_request(stdin: TextIO) -> tuple[str, int]:
    try:
        request = json.loads(stdin.read())
    except ValueError:
        raise ValueError("请求不是合法的 JSON") from None
    if not isinstance(request, dict):
        raise ValueError("请求必须是 JSON 对象")
    query, max_results = request.get("query"), request.get("max_results")
    if not isinstance(query, str) or not query.strip():
        raise ValueError("query 必须是非空字符串")
    if isinstance(max_results, bool) or not isinstance(max_results, int):
        raise ValueError("max_results 必须是整数")
    return query, max_results


def main(stdin: TextIO, stdout: TextIO) -> int:
    try:
        query, max_results = _parse_request(stdin)
    except ValueError as exc:
        return _fail(stdout, str(exc))
    try:
        from ddgs import DDGS
    except ImportError:
        return _fail(stdout, "未安装 ddgs")
    results, last_error = [], None
    for attempt in range(MAX_ATTEMPTS):
        if attempt:
            time.sleep(RETRY_DELAY)
        try:
            results = DDGS().text(query, max_results=max_results)
        except Exception as exc:  # ddgs 的异常类型较多，统一转成错误信息
            if "No results found" in str(exc):
                results, last_error = [], None
            elif type(exc).__name__ in _TRANSIENT_ERRORS:
                results, last_error = [], f"{type(exc).__name__}: {exc}"
            else:
                return _fail(stdout, f"{type(exc).__name__}: {exc}")
        else:
            last_error = None
        if results:
            break
    if not results and last_error:
        return _fail(stdout, last_error)
    json.dump(results, stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.stdin, sys.stdout))
