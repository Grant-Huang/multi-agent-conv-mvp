"""给 agent 用的网页搜索与抓取工具。"""
from .errors import (
    BlockedUrl,
    FetchFailed,
    InvalidArgument,
    ProviderError,
    Timeout,
    WebSearchError,
)
from .models import FetchFailure, Page, SearchAndFetchResult, SearchResult
from .settings import Settings

__all__ = [
    "BlockedUrl",
    "FetchFailed",
    "FetchFailure",
    "InvalidArgument",
    "Page",
    "ProviderError",
    "SearchAndFetchResult",
    "SearchResult",
    "Settings",
    "Timeout",
    "WebSearchError",
]
