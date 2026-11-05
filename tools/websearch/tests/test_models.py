import dataclasses

import pytest

from agent_websearch.models import FetchFailure, Page, SearchAndFetchResult, SearchResult


def make_result(rank=1):
    return SearchResult(
        title="标题", url=f"https://example.com/{rank}", snippet="摘要", provider="ddg", rank=rank
    )


def make_page():
    return Page(
        url="https://example.com/1",
        final_url="https://example.com/1/",
        title="标题",
        content="# 正文",
        method="local",
        chars=4,
        truncated=False,
        elapsed_ms=12,
    )


def test_search_result_to_dict():
    assert make_result().to_dict() == {
        "title": "标题",
        "url": "https://example.com/1",
        "snippet": "摘要",
        "provider": "ddg",
        "rank": 1,
    }


def test_page_to_dict():
    d = make_page().to_dict()
    assert d["final_url"] == "https://example.com/1/"
    assert d["method"] == "local"
    assert d["truncated"] is False
    assert set(d) == {
        "url", "final_url", "title", "content", "method", "chars", "truncated", "elapsed_ms"
    }


def test_fetch_failure_to_dict():
    f = FetchFailure(url="https://example.com/x", code="timeout", message="超时")
    assert f.to_dict() == {"url": "https://example.com/x", "code": "timeout", "message": "超时"}


def test_search_and_fetch_result_to_dict_is_nested_plain_data():
    r = SearchAndFetchResult(
        query="q",
        provider="ddg",
        results=[make_result(1), make_result(2)],
        pages=[make_page()],
        failures=[FetchFailure(url="https://example.com/2", code="fetch_failed", message="x")],
    )
    d = r.to_dict()
    assert d["query"] == "q"
    assert [x["rank"] for x in d["results"]] == [1, 2]
    assert d["pages"][0]["method"] == "local"
    assert d["failures"][0]["code"] == "fetch_failed"


def test_models_are_frozen():
    with pytest.raises(dataclasses.FrozenInstanceError):
        make_result().title = "改"
    with pytest.raises(dataclasses.FrozenInstanceError):
        make_page().content = "改"
