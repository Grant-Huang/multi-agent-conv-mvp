import json

import httpx
import pytest

from agent_websearch import providers
from agent_websearch.errors import ProviderError, Timeout
from agent_websearch.models import SearchResult
from agent_websearch.providers import parallel

URL = "https://search.parallel.ai/mcp"


def body(results):
    inner = json.dumps({"results": results}, ensure_ascii=False)
    payload = {"result": {"content": [{"type": "text", "text": inner}]}}
    return json.dumps(payload)


def test_parallel_is_registered():
    assert providers.get_provider("parallel").name == "parallel"


def test_parses_title_url_and_excerpts(mock_http):
    route = mock_http.post(URL).mock(return_value=httpx.Response(200, text=body([
        {"title": "KV 缓存", "url": "https://example.com/kv", "excerpts": ["槽位共享", "同一块 KV"]},
        {"title": "第二篇", "url": "https://example.com/two", "excerpts": ["第二段"]},
    ])))
    results = parallel.ParallelProvider().search("llama.cpp kv", 5, 12)
    assert results == [
        SearchResult("KV 缓存", "https://example.com/kv", "槽位共享 同一块 KV", "parallel", 1),
        SearchResult("第二篇", "https://example.com/two", "第二段", "parallel", 2),
    ]
    sent = json.loads(route.calls.last.request.content)
    assert sent["params"]["name"] == "web_search"
    args = sent["params"]["arguments"]
    assert args["objective"] == "llama.cpp kv"
    assert args["search_queries"] == ["llama.cpp kv"]
    assert "authorization" not in route.calls.last.request.headers


def test_caps_results_and_skips_bad_urls(mock_http):
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=body([
        {"title": "好", "url": "https://ok.example/a", "excerpts": ["好的"]},
        {"title": "坏", "url": "javascript:alert(1)", "excerpts": ["不要"]},
        {"title": "重复", "url": "https://ok.example/a", "excerpts": ["又一次"]},
        {"title": "多", "url": "https://ok.example/b", "excerpts": ["第二条"]},
    ])))
    results = parallel.ParallelProvider().search("q", 1, 5)
    assert [r.url for r in results] == ["https://ok.example/a"]


def test_empty_results(mock_http):
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=body([])))
    assert parallel.ParallelProvider().search("q", 3, 5) == []


def test_timeout(mock_http):
    mock_http.post(URL).mock(side_effect=httpx.TimeoutException("slow"))
    with pytest.raises(Timeout, match="Parallel"):
        parallel.ParallelProvider().search("q", 3, 5)


@pytest.mark.parametrize("status", [401, 429, 500])
def test_http_status_is_provider_error(mock_http, status):
    mock_http.post(URL).mock(return_value=httpx.Response(status, text="query leaked"))
    with pytest.raises(ProviderError, match=f"HTTP {status}") as info:
        parallel.ParallelProvider().search("秘密查询", 3, 5)
    assert "秘密查询" not in str(info.value)
    assert "query leaked" not in str(info.value)


def test_mcp_error_message(mock_http):
    payload = {"error": {"code": -32000, "message": "rate limited"}}
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=json.dumps(payload)))
    with pytest.raises(ProviderError, match="rate limited"):
        parallel.ParallelProvider().search("q", 3, 5)


def test_unparseable_body(mock_http):
    mock_http.post(URL).mock(return_value=httpx.Response(200, text="not-json"))
    with pytest.raises(ProviderError, match="无法解析"):
        parallel.ParallelProvider().search("q", 3, 5)
