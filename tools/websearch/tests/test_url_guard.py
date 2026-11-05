import socket

import pytest

from agent_websearch import url_guard
from agent_websearch.errors import BlockedUrl, InvalidArgument
from agent_websearch.url_guard import check_url


def resolve_to(monkeypatch, *ips):
    def fake(host, port, *args, **kwargs):
        out = []
        for ip in ips:
            family = socket.AF_INET6 if ":" in ip else socket.AF_INET
            sockaddr = (ip, 0, 0, 0) if ":" in ip else (ip, 0)
            out.append((family, socket.SOCK_STREAM, 6, "", sockaddr))
        return out

    monkeypatch.setattr(url_guard.socket, "getaddrinfo", fake)


def test_public_host_is_allowed(monkeypatch):
    resolve_to(monkeypatch, "93.184.216.34")
    check_url("https://example.com/path?q=1")


def test_public_ipv6_host_is_allowed(monkeypatch):
    resolve_to(monkeypatch, "2606:2800:220:1:248:1893:25c8:1946")
    check_url("http://example.com/")


@pytest.mark.parametrize("url", [
    "http://127.0.0.1/",
    "http://127.1.2.3:8080/",
    "http://10.0.0.5/",
    "http://172.16.0.1/",
    "http://192.168.1.1:8080/admin",
    "http://169.254.169.254/latest/meta-data",
    "http://0.0.0.0/",
    "http://100.64.0.1/",
    "http://224.0.0.1/",
    "http://[::1]/",
    "http://[fe80::1]/",
    "http://[fc00::1]/",
    "http://[::ffff:127.0.0.1]/",
])
def test_private_ip_literals_are_blocked(url):
    with pytest.raises(BlockedUrl):
        check_url(url)


def test_localhost_resolving_to_loopback_is_blocked(monkeypatch):
    resolve_to(monkeypatch, "127.0.0.1")
    with pytest.raises(BlockedUrl):
        check_url("http://localhost:8200/v1/models")


def test_any_private_address_among_several_blocks(monkeypatch):
    resolve_to(monkeypatch, "93.184.216.34", "10.1.1.1")
    with pytest.raises(BlockedUrl):
        check_url("https://mixed.example.com/")


def test_ipv6_zone_id_is_handled(monkeypatch):
    def fake(host, port, *args, **kwargs):
        return [(socket.AF_INET6, socket.SOCK_STREAM, 6, "", ("fe80::1%en0", 0, 0, 0))]

    monkeypatch.setattr(url_guard.socket, "getaddrinfo", fake)
    with pytest.raises(BlockedUrl):
        check_url("http://zone.example.com/")


def test_unresolvable_host_is_not_blocked(monkeypatch):
    def fake(host, port, *args, **kwargs):
        raise socket.gaierror("no such host")

    monkeypatch.setattr(url_guard.socket, "getaddrinfo", fake)
    check_url("http://does-not-exist.invalid/")


def test_allow_private_lets_internal_addresses_through():
    check_url("http://127.0.0.1:8200/", allow_private=True)
    check_url("http://[::1]/", allow_private=True)


@pytest.mark.parametrize("url", [
    "file:///etc/passwd",
    "ftp://example.com/a",
    "javascript:alert(1)",
    "gopher://example.com/",
])
def test_non_http_schemes_are_blocked(url):
    with pytest.raises(BlockedUrl):
        check_url(url)


def test_scheme_is_still_enforced_with_allow_private():
    with pytest.raises(BlockedUrl):
        check_url("file:///etc/passwd", allow_private=True)


@pytest.mark.parametrize("url", [
    "",
    "   ",
    "example.com/path",
    "http://",
    "https:///nohost",
    "http://example.com/\nHost: evil.com",
    "http://exa mple.com/",
    "http://example.com/" + "a" * 2048,
])
def test_malformed_urls_are_invalid(url):
    with pytest.raises(InvalidArgument):
        check_url(url)


def test_non_string_is_invalid():
    with pytest.raises(InvalidArgument):
        check_url(None)
