import json

import httpx
import pytest

from agent_websearch import providers
from agent_websearch.errors import ProviderError, Timeout
from agent_websearch.models import SearchResult
from agent_websearch.providers import exa

URL = "https://mcp.exa.ai/mcp"
PAGE = (
    "Title: KV 缓存说明\n"
    "URL: https://example.com/kv\n"
    "Published: N/A\n"
    "Author: N/A\n"
    "Highlights:\n"
    "槽位共享同一块 KV。\n"
    "\n---\n\n"
    "Title: 第二篇\n"
    "URL: https://example.com/two\n"
    "Published: 2025-05-17T00:00:00.000Z\n"
    "Author: N/A\n"
    "Highlights:\n"
    "第二段摘要。\n"
)


def sse(text):
    payload = {"result": {"content": [{"type": "text", "text": text}]}}
    return "event: message\ndata: " + json.dumps(payload, ensure_ascii=False)


def test_exa_is_registered():
    assert providers.get_provider("exa").name == "exa"


def test_parses_title_url_and_highlights(mock_http):
    route = mock_http.post(URL).mock(return_value=httpx.Response(200, text=sse(PAGE)))
    results = exa.ExaProvider().search("llama.cpp kv", 5, 12)
    assert results == [
        SearchResult("KV 缓存说明", "https://example.com/kv", "槽位共享同一块 KV。", "exa", 1),
        SearchResult("第二篇", "https://example.com/two", "第二段摘要。", "exa", 2),
    ]
    sent = json.loads(route.calls.last.request.content)
    assert sent["params"]["name"] == "web_search_exa"
    assert sent["params"]["arguments"]["query"] == "llama.cpp kv"
    assert sent["params"]["arguments"]["numResults"] == 5
    assert "authorization" not in route.calls.last.request.headers


def test_caps_results_and_skips_bad_urls(mock_http):
    text = (
        "Title: 好\nURL: https://ok.example/a\nHighlights:\n好的\n\n---\n\n"
        "Title: 坏\nURL: javascript:alert(1)\nHighlights:\n不要\n\n---\n\n"
        "Title: 重复\nURL: https://ok.example/a\nHighlights:\n又一次\n\n---\n\n"
        "Title: 多\nURL: https://ok.example/b\nHighlights:\n第二条\n"
    )
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=sse(text)))
    results = exa.ExaProvider().search("q", 1, 5)
    assert [r.url for r in results] == ["https://ok.example/a"]


def test_no_matches_returns_empty(mock_http):
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=sse("No results.")))
    assert exa.ExaProvider().search("q", 3, 5) == []


def test_timeout(mock_http):
    mock_http.post(URL).mock(side_effect=httpx.TimeoutException("slow"))
    with pytest.raises(Timeout, match="Exa"):
        exa.ExaProvider().search("q", 3, 5)


@pytest.mark.parametrize("status", [401, 429, 500])
def test_http_status_is_provider_error(mock_http, status):
    mock_http.post(URL).mock(return_value=httpx.Response(status, text="query leaked"))
    with pytest.raises(ProviderError, match=f"HTTP {status}") as info:
        exa.ExaProvider().search("秘密查询", 3, 5)
    assert "秘密查询" not in str(info.value)
    assert "query leaked" not in str(info.value)


def test_mcp_error_message(mock_http):
    payload = {"error": {"code": -32000, "message": "rate limited"}}
    raw = "event: message\ndata: " + json.dumps(payload)
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=raw))
    with pytest.raises(ProviderError, match="rate limited"):
        exa.ExaProvider().search("q", 3, 5)


def test_unparseable_body(mock_http):
    raw = "event: message\ndata: not-json"
    mock_http.post(URL).mock(return_value=httpx.Response(200, text=raw))
    with pytest.raises(ProviderError, match="无法解析"):
        exa.ExaProvider().search("q", 3, 5)
