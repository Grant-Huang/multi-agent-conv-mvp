"""统一异常。`code` 会原样出现在 CLI / MCP 的 JSON 输出里，修改需同步调用方。"""


class WebSearchError(Exception):
    code = "internal"

    def __init__(self, message: str):
        super().__init__(message)
        self.message = message


class InvalidArgument(WebSearchError):
    code = "invalid_argument"


class BlockedUrl(WebSearchError):
    code = "blocked_url"


class Timeout(WebSearchError):
    code = "timeout"


class ProviderError(WebSearchError):
    code = "provider_error"


class FetchFailed(WebSearchError):
    code = "fetch_failed"
