"""Jina Reader（r.jina.ai）：由对方抓取网页并返回 Markdown。

会把 URL 发给第三方；调用前 api 已做过 URL 校验，内网地址不会走到这里。
JINA_API_KEY 只用于请求头，不写日志，也不进错误信息。
"""
import re

import httpx

from .errors import FetchFailed, Timeout
from .models import Fetched
from .settings import Settings

JINA_PREFIX = "https://r.jina.ai/"
CONTENT_MARKER = "Markdown Content:"
_UPSTREAM_ERROR = re.compile(r"^Warning: Target URL returned error (\d+)", re.M)
_TITLE = re.compile(r"^Title:[ \t]*(.*)$", re.M)
_SOURCE = re.compile(r"^URL Source:[ \t]*(\S+)", re.M)


def fetch_jina(url: str, settings: Settings) -> Fetched:
    headers = {}
    if settings.jina_api_key:
        headers["Authorization"] = f"Bearer {settings.jina_api_key}"
    try:
        response = httpx.get(JINA_PREFIX + url, headers=headers,
                             timeout=settings.jina_timeout, follow_redirects=True)
    except httpx.TimeoutException:
        raise Timeout(f"Jina 超时（{settings.jina_timeout:g}s）") from None
    except httpx.HTTPError as exc:
        raise FetchFailed(f"Jina 请求失败：{type(exc).__name__}") from None
    if response.status_code != 200:
        raise FetchFailed(f"Jina HTTP {response.status_code}")
    text = response.text[:settings.max_bytes]
    upstream = _UPSTREAM_ERROR.search(text)
    if upstream:
        raise FetchFailed(f"目标网站返回 {upstream.group(1)}（经 Jina）")
    title = _TITLE.search(text)
    source = _SOURCE.search(text)
    marker = text.find(CONTENT_MARKER)
    content = (text[marker + len(CONTENT_MARKER):] if marker >= 0 else text).strip()
    if len(content) < settings.min_chars:
        raise FetchFailed(f"Jina 返回的正文过短（{len(content)} 字）")
    return Fetched(
        source.group(1) if source else url,
        title.group(1).strip() if title else "",
        content,
    )
