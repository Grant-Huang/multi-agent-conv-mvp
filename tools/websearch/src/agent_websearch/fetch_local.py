"""本机抓取：httpx 流式下载 → 按内容类型处理 → HTML 转 Markdown。

重定向由这里手动跟随，每一跳都重新做 URL 校验，防止被重定向到内网。
"""
import logging
import re
from urllib.parse import urljoin

import httpx

from .errors import FetchFailed, Timeout
from .extract import html_to_markdown
from .models import Fetched
from .settings import Settings
from .url_guard import check_url

logger = logging.getLogger("agent_websearch")

MAX_REDIRECTS = 5
REDIRECT_STATUS = {301, 302, 303, 307, 308}
HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/130.0 Safari/537.36"
    ),
    "Accept": "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
}
HTML_TYPES = {"text/html", "application/xhtml+xml", ""}
TEXT_TYPES = {"text/plain", "text/markdown", "application/json"}
_META_CHARSET = re.compile(rb"<meta[^>]+charset\s*=\s*[\"']?\s*([\w-]+)", re.I)
_HEADER_CHARSET = re.compile(r"charset\s*=\s*[\"']?([\w-]+)", re.I)


def fetch_local(url: str, settings: Settings) -> Fetched:
    current = url
    with httpx.Client(timeout=settings.fetch_timeout, follow_redirects=False,
                      headers=HEADERS) as client:
        for _ in range(MAX_REDIRECTS + 1):
            check_url(current, settings.allow_private)
            try:
                with client.stream("GET", current) as response:
                    if response.status_code in REDIRECT_STATUS:
                        location = response.headers.get("location")
                        if not location:
                            raise FetchFailed("重定向响应缺少 Location")
                        current = urljoin(current, location)
                        continue
                    return _handle(response, current, settings)
            except httpx.TimeoutException:
                raise Timeout(f"本机抓取超时（{settings.fetch_timeout:g}s）") from None
            except httpx.HTTPError as exc:
                raise FetchFailed(f"本机抓取失败：{type(exc).__name__}") from None
    raise FetchFailed(f"重定向次数过多（超过 {MAX_REDIRECTS} 次）")


def _handle(response: httpx.Response, final_url: str, settings: Settings) -> Fetched:
    if response.status_code >= 400:
        raise FetchFailed(f"HTTP {response.status_code}")
    content_type_header = response.headers.get("content-type", "")
    content_type = content_type_header.split(";")[0].strip().lower()
    if content_type not in HTML_TYPES and content_type not in TEXT_TYPES:
        raise FetchFailed(f"不支持的内容类型：{content_type}")
    data = _read_capped(response, settings.max_bytes)
    text = _decode(data, content_type_header)
    if content_type in TEXT_TYPES:
        title, content = "", text.strip()
    else:
        title, content = html_to_markdown(text, final_url, settings.extractor)
    if len(content) < settings.min_chars:
        raise FetchFailed(f"正文过短（{len(content)} 字）")
    return Fetched(final_url, title, content)


def _read_capped(response: httpx.Response, max_bytes: int) -> bytes:
    data = bytearray()
    for chunk in response.iter_bytes():
        data.extend(chunk)
        if len(data) >= max_bytes:
            break
    return bytes(data[:max_bytes])


def _decode(data: bytes, content_type_header: str) -> str:
    header = _HEADER_CHARSET.search(content_type_header)
    meta = _META_CHARSET.search(data[:2048])
    declared = [header.group(1)] if header else []
    if meta:
        declared.append(meta.group(1).decode("ascii", "ignore"))
    for encoding in declared:
        try:
            return data.decode(encoding)
        except (LookupError, UnicodeDecodeError):
            continue
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return data.decode("gb18030", errors="replace")
