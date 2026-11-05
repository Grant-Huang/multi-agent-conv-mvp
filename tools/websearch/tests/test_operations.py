import pytest

from agent_websearch import api, operations
from agent_websearch.errors import BlockedUrl, Timeout
from agent_websearch.models import FetchFailure, Page, SearchAndFetchResult, SearchResult


def result(rank=1):
    return SearchResult(f"标题{rank}", f"https://a.com/{rank}", "摘要", "ddg", rank)


def page(url="https://a.com/1"):
    return Page(url, url, "标题", "正文", "local", 2, False, 9)


@pytest.fixture(autouse=True)
def env(clean_env):
    return clean_env


def test_search_envelope(monkeypatch):
    seen = {}

    def fake_search(query, **kwargs):
        seen.update(query=query, **kwargs)
        return [result(1), result(2)]

    monkeypatch.setattr(api, "search", fake_search)
    env = operations.run("search", query="瀑布模型", max_results=5, provider="ddg")
    assert env["status"] == "success"
    assert env["message"] == ""
    assert env["data"]["query"] == "瀑布模型"
    assert env["data"]["provider"] == "ddg"
    assert [r["rank"] for r in env["data"]["results"]] == [1, 2]
    assert seen["max_results"] == 5 and seen["provider"] == "ddg"


def test_search_provider_defaults_to_settings_when_no_results(monkeypatch):
    monkeypatch.setenv("WEBSEARCH_PROVIDER", "exa")
    monkeypatch.setattr(api, "search", lambda query, **kwargs: [])
    env = operations.run("search", query="q", max_results=3)
    assert env["data"] == {"query": "q", "provider": "exa", "results": []}


def test_fetch_envelope(monkeypatch):
    monkeypatch.setattr(api, "fetch", lambda url, **kwargs: page(url))
    env = operations.run("fetch", url="https://a.com/1", max_chars=1000)
    assert env["status"] == "success"
    assert env["data"]["content"] == "正文"
    assert env["data"]["method"] == "local"


def test_search_and_fetch_envelope_without_failures(monkeypatch):
    out = SearchAndFetchResult("q", "ddg", [result(1)], [page()], [])
    monkeypatch.setattr(api, "search_and_fetch", lambda query, **kwargs: out)
    env = operations.run("search_and_fetch", query="q", max_results=3, max_chars_per_page=3000)
    assert env["status"] == "success"
    assert env["message"] == ""
    assert env["data"]["pages"][0]["url"] == "https://a.com/1"


def test_search_and_fetch_reports_failed_pages_in_message(monkeypatch):
    failures = [FetchFailure("https://a.com/2", "timeout", "超时"),
                FetchFailure("https://a.com/3", "fetch_failed", "坏")]
    out = SearchAndFetchResult("q", "ddg", [result(1)], [page()], failures)
    monkeypatch.setattr(api, "search_and_fetch", lambda query, **kwargs: out)
    env = operations.run("search_and_fetch", query="q", max_results=3, max_chars_per_page=3000)
    assert env["status"] == "success"
    assert "2 个页面抓取失败" in env["message"]
    assert env["data"]["failures"][0]["code"] == "timeout"


def test_search_and_fetch_passes_all_arguments(monkeypatch):
    seen = {}

    def fake(query, **kwargs):
        seen.update(query=query, **kwargs)
        return SearchAndFetchResult(query, "ddg", [], [], [])

    monkeypatch.setattr(api, "search_and_fetch", fake)
    operations.run("search_and_fetch", query="q", max_results=4, max_chars_per_page=1500,
                   provider="ddg")
    assert (seen["max_results"], seen["max_chars_per_page"], seen["provider"]) == (4, 1500, "ddg")


@pytest.mark.parametrize("error", [BlockedUrl("不允许访问内网地址"), Timeout("超时")])
def test_known_errors_become_error_envelope(monkeypatch, error):
    def boom(url, **kwargs):
        raise error

    monkeypatch.setattr(api, "fetch", boom)
    env = operations.run("fetch", url="http://x", max_chars=1000)
    assert env == {"status": "error", "data": {"code": error.code}, "message": error.message}


def test_unexpected_errors_hide_details(monkeypatch):
    def boom(url, **kwargs):
        raise RuntimeError("secret detail /Users/me/key")

    monkeypatch.setattr(api, "fetch", boom)
    env = operations.run("fetch", url="http://x", max_chars=1000)
    assert env["status"] == "error"
    assert env["data"] == {"code": "internal"}
    assert "RuntimeError" in env["message"]
    assert "secret detail" not in env["message"]


def test_unknown_operation_is_an_internal_error():
    env = operations.run("bogus")
    assert env["status"] == "error"
    assert env["data"]["code"] == "internal"
