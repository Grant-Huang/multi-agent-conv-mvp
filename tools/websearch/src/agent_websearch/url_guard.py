"""URL 校验：只允许 http/https，拒绝指向内网、本机等非公网地址的 URL（防 SSRF）。

已知限制：校验时的 DNS 解析结果与真正连接时可能不同（DNS 重绑定），v1 不处理。
"""
import ipaddress
import re
import socket
from urllib.parse import urlsplit

from .errors import BlockedUrl, InvalidArgument

MAX_URL_LENGTH = 2048
ALLOWED_SCHEMES = ("http", "https")
_BAD_CHARS = re.compile(r"[\s\x00-\x1f\x7f]")


def check_url(url: str, allow_private: bool = False) -> None:
    """不合法抛 InvalidArgument / BlockedUrl，合法则无返回。"""
    if not isinstance(url, str):
        raise InvalidArgument("url 必须是字符串")
    url = url.strip()
    if not url or len(url) > MAX_URL_LENGTH or _BAD_CHARS.search(url):
        raise InvalidArgument("url 为空、过长或含有空白/控制字符")
    try:
        parts = urlsplit(url)
        host = parts.hostname
    except ValueError:
        raise InvalidArgument("url 格式不正确") from None
    if not parts.scheme:
        raise InvalidArgument("url 缺少协议（需要 http:// 或 https://）")
    if parts.scheme.lower() not in ALLOWED_SCHEMES:
        raise BlockedUrl(f"只允许 http/https，不允许 {parts.scheme}")
    if not host:
        raise InvalidArgument("url 缺少主机名")
    if allow_private:
        return
    for ip in _resolve(host):
        if _is_blocked(ip):
            raise BlockedUrl("不允许访问内网、本机或保留地址")


def _resolve(host: str) -> list[ipaddress.IPv4Address | ipaddress.IPv6Address]:
    try:
        return [ipaddress.ip_address(host.split("%")[0])]
    except ValueError:
        pass
    try:
        infos = socket.getaddrinfo(host, None, type=socket.SOCK_STREAM)
    except OSError:
        # 本机解析不了就连不上，由抓取环节自己报错；交给 Jina 时由对方的网络解析，不涉及本机内网
        return []
    addresses = []
    for info in infos:
        try:
            addresses.append(ipaddress.ip_address(info[4][0].split("%")[0]))
        except ValueError:
            continue
    return addresses


def _is_blocked(ip: ipaddress.IPv4Address | ipaddress.IPv6Address) -> bool:
    if isinstance(ip, ipaddress.IPv6Address) and ip.ipv4_mapped is not None:
        ip = ip.ipv4_mapped
    return (
        not ip.is_global
        or ip.is_private
        or ip.is_loopback
        or ip.is_link_local
        or ip.is_multicast
        or ip.is_reserved
        or ip.is_unspecified
    )
