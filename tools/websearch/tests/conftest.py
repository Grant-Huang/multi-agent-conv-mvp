import logging
import os
import socket

import pytest
import respx

from agent_websearch import url_guard


@pytest.fixture
def clean_env(monkeypatch):
    """去掉会影响 Settings 的环境变量，保证测试结果与本机环境无关。"""
    for key in list(os.environ):
        if key.startswith("WEBSEARCH_") or key == "JINA_API_KEY":
            monkeypatch.delenv(key, raising=False)
    return monkeypatch


@pytest.fixture(autouse=True)
def public_dns(request, monkeypatch):
    """默认让所有域名解析到一个公网地址，测试不依赖真实 DNS（live 测试用真实 DNS）。"""
    if request.node.get_closest_marker("live"):
        return

    def fake(host, port, *args, **kwargs):
        return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 0))]

    monkeypatch.setattr(url_guard.socket, "getaddrinfo", fake)


@pytest.fixture
def mock_http():
    """拦截 httpx 请求；没有匹配路由的请求会报错，不会真的联网。"""
    with respx.mock(assert_all_called=False) as router:
        yield router


@pytest.fixture(autouse=True)
def restore_logger():
    """configure_logging 会改 logger 的 handler / 级别 / 传播，每个测试后还原，避免互相影响。"""
    logger = logging.getLogger("agent_websearch")
    saved = (list(logger.handlers), logger.level, logger.propagate)
    yield
    logger.handlers[:] = saved[0]
    logger.setLevel(saved[1])
    logger.propagate = saved[2]


@pytest.fixture(autouse=True)
def no_retry_delay(monkeypatch):
    """搜索子进程的重试间隔在测试里归零，避免白等。"""
    from agent_websearch.providers import _ddg_worker

    monkeypatch.setattr(_ddg_worker, "RETRY_DELAY", 0)
