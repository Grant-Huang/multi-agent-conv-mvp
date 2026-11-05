import threading
import time

import pytest

from agent_websearch import api, providers
from agent_websearch.errors import FetchFailed, InvalidArgument, ProviderError, Timeout
from agent_websearch.models import Page, SearchResult
from agent_websearch.settings import Settings


class FakeProvider:
    name = "fake"

    def __init__(self, results=None, error=None):
        self.results = results if results is not None else []
        self.error = error
        self.calls = []

    def search(self, query, max_results, timeout):
        self.calls.append((query, max_results, timeout))
        if self.error:
            raise self.error
        return self.results[:max_results]


def result(rank):
    return SearchResult(f"标题{rank}", f"https://example.com/{rank}", f"摘要{rank}", "fake", rank)


def page(url, content="正文", method="local"):
    return Page(url, url, "t", content, method, len(content), False, 5)


@pytest.fixture
def fake(monkeypatch):
    provider = FakeProvider([result(i) for i in range(1, 9)])
    monkeypatch.setattr(providers, "_REGISTRY", {"fake": provider})
    return provider


SETTINGS = Settings(provider="fake", search_timeout=7)


# --- search ---

def test_search_returns_provider_results(fake):
    results = api.search("瀑布模型", max_results=3, settings=SETTINGS)
    assert [r.rank for r in results] == [1, 2, 3]
    assert fake.calls == [("瀑布模型", 3, 7)]


def test_query_is_stripped(fake):
    api.search("  瀑布模型  ", settings=SETTINGS)
    assert fake.calls[0][0] == "瀑布模型"


def test_default_max_results_is_8(fake):
    assert len(api.search("q", settings=SETTINGS)) == 8


def test_provider_argument_overrides_settings(monkeypatch, fake):
    other = FakeProvider([result(1)])
    monkeypatch.setattr(providers, "_REGISTRY", {"fake": fake, "other": other})
    assert len(api.search("q", provider="other", settings=SETTINGS)) == 1
    assert other.calls and not fake.calls


@pytest.mark.parametrize("query", ["", "   ", None, 123, "x" * 501])
def test_search_rejects_bad_query(fake, query):
    with pytest.raises(InvalidArgument):
        api.search(query, settings=SETTINGS)
    assert fake.calls == []


@pytest.mark.parametrize("max_results", [0, 21, -1, "3", None, True, 2.5])
def test_search_rejects_bad_max_results(fake, max_results):
    with pytest.raises(InvalidArgument):
        api.search("q", max_results=max_results, settings=SETTINGS)


@pytest.mark.parametrize("max_results", [1, 20])
def test_search_accepts_boundary_max_results(fake, max_results):
    api.search("q", max_results=max_results, settings=SETTINGS)


def test_query_of_exactly_500_chars_is_accepted(fake):
    api.search("x" * 500, settings=SETTINGS)


def test_unknown_provider_is_invalid_argument(fake):
    with pytest.raises(InvalidArgument):
        api.search("q", provider="nope", settings=SETTINGS)


def test_provider_errors_propagate(monkeypatch):
    monkeypatch.setattr(providers, "_REGISTRY", {"fake": FakeProvider(error=ProviderError("限流"))})
    with pytest.raises(ProviderError, match="限流"):
        api.search("q", settings=SETTINGS)


# --- search_and_fetch ---

def stub_fetch(monkeypatch, behaviour=None, delays=None):
    calls = []
    lock = threading.Lock()

    def fetch(url, max_chars, settings):
        with lock:
            calls.append((url, max_chars))
        time.sleep((delays or {}).get(url, 0))
        outcome = (behaviour or {}).get(url)
        if isinstance(outcome, Exception):
            raise outcome
        return outcome or page(url)

    monkeypatch.setattr(api, "fetch", fetch)
    return calls


def test_returns_pages_in_rank_order_even_if_completion_order_differs(fake, monkeypatch):
    calls = stub_fetch(monkeypatch, delays={"https://example.com/1": 0.15})
    out = api.search_and_fetch("q", max_results=3, settings=SETTINGS)
    assert [p.url for p in out.pages] == [f"https://example.com/{i}" for i in (1, 2, 3)]
    assert [r.rank for r in out.results] == [1, 2, 3]
    assert (out.query, out.provider, out.failures) == ("q", "fake", [])
    assert len(calls) == 3


def test_pages_are_fetched_concurrently(fake, monkeypatch):
    delays = {f"https://example.com/{i}": 0.3 for i in range(1, 4)}
    stub_fetch(monkeypatch, delays=delays)
    started = time.monotonic()
    api.search_and_fetch("q", max_results=3, settings=SETTINGS)
    assert time.monotonic() - started < 0.8


def test_default_sizes_are_3_results_and_3000_chars_each(fake, monkeypatch):
    calls = stub_fetch(monkeypatch)
    out = api.search_and_fetch("q", settings=SETTINGS)
    assert len(out.pages) == 3
    assert {max_chars for _, max_chars in calls} == {3000}


def test_max_chars_per_page_is_passed_through(fake, monkeypatch):
    calls = stub_fetch(monkeypatch)
    api.search_and_fetch("q", max_results=2, max_chars_per_page=1234, settings=SETTINGS)
    assert {max_chars for _, max_chars in calls} == {1234}


def test_failed_pages_are_listed_not_raised(fake, monkeypatch):
    stub_fetch(monkeypatch, behaviour={
        "https://example.com/2": FetchFailed("local: HTTP 521；jina: 超时"),
        "https://example.com/3": Timeout("超时"),
    })
    out = api.search_and_fetch("q", max_results=3, settings=SETTINGS)
    assert [p.url for p in out.pages] == ["https://example.com/1"]
    failures = {f.url: (f.code, f.message) for f in out.failures}
    assert failures["https://example.com/2"] == ("fetch_failed", "local: HTTP 521；jina: 超时")
    assert failures["https://example.com/3"] == ("timeout", "超时")
    assert [f.url for f in out.failures] == ["https://example.com/2", "https://example.com/3"]


def test_unexpected_exception_in_one_page_does_not_break_the_batch(fake, monkeypatch):
    stub_fetch(monkeypatch, behaviour={"https://example.com/1": RuntimeError("boom 内部细节")})
    out = api.search_and_fetch("q", max_results=2, settings=SETTINGS)
    assert len(out.pages) == 1
    (failure,) = out.failures
    assert failure.code == "internal"
    assert "RuntimeError" in failure.message
    assert "内部细节" not in failure.message


def test_all_pages_failing_still_returns_search_results(fake, monkeypatch):
    stub_fetch(monkeypatch, behaviour={
        f"https://example.com/{i}": FetchFailed("坏") for i in range(1, 4)
    })
    out = api.search_and_fetch("q", settings=SETTINGS)
    assert out.pages == []
    assert len(out.failures) == 3
    assert len(out.results) == 3


def test_no_search_results_means_no_fetching(monkeypatch):
    monkeypatch.setattr(providers, "_REGISTRY", {"fake": FakeProvider([])})
    calls = stub_fetch(monkeypatch)
    out = api.search_and_fetch("q", settings=SETTINGS)
    assert (out.results, out.pages, out.failures) == ([], [], [])
    assert calls == []


def test_search_failure_propagates(monkeypatch):
    monkeypatch.setattr(providers, "_REGISTRY", {"fake": FakeProvider(error=Timeout("搜索超时"))})
    calls = stub_fetch(monkeypatch)
    with pytest.raises(Timeout):
        api.search_and_fetch("q", settings=SETTINGS)
    assert calls == []


@pytest.mark.parametrize("max_results", [0, 9, True, "3"])
def test_search_and_fetch_limits_max_results_to_8(fake, monkeypatch, max_results):
    calls = stub_fetch(monkeypatch)
    with pytest.raises(InvalidArgument):
        api.search_and_fetch("q", max_results=max_results, settings=SETTINGS)
    assert calls == []


@pytest.mark.parametrize("max_chars", [499, 100001, None])
def test_search_and_fetch_validates_page_size_before_searching(fake, monkeypatch, max_chars):
    with pytest.raises(InvalidArgument):
        api.search_and_fetch("q", max_chars_per_page=max_chars, settings=SETTINGS)
    assert fake.calls == []


def test_only_requested_number_of_pages_is_fetched(fake, monkeypatch):
    calls = stub_fetch(monkeypatch)
    api.search_and_fetch("q", max_results=8, settings=SETTINGS)
    assert len(calls) == 8


def test_result_to_dict_is_plain_data(fake, monkeypatch):
    stub_fetch(monkeypatch)
    data = api.search_and_fetch("q", max_results=1, settings=SETTINGS).to_dict()
    assert data["pages"][0]["method"] == "local"
    assert data["results"][0]["provider"] == "fake"
