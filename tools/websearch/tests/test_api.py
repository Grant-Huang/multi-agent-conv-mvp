import pytest

from agent_websearch import api
from agent_websearch.errors import BlockedUrl, FetchFailed, InvalidArgument, Timeout
from agent_websearch.models import Fetched, Page
from agent_websearch.settings import Settings

URL = "https://example.com/post"
CONTENT = "第一段内容。\n\n第二段内容。\n\n第三段内容。"


class FakeFetchers:
    """替换 api 里的两个抓取实现，记录调用顺序。"""

    def __init__(self, monkeypatch, local=None, jina=None):
        self.calls = []
        self._behaviour = {"local": local, "jina": jina}
        monkeypatch.setattr(api, "fetch_local", self._make("local"))
        monkeypatch.setattr(api, "fetch_jina", self._make("jina"))

    def _make(self, name):
        def fetcher(url, settings):
            self.calls.append(name)
            behaviour = self._behaviour[name]
            if isinstance(behaviour, Exception):
                raise behaviour
            return behaviour or Fetched(url, f"{name}标题", CONTENT)
        return fetcher


# --- fetch：参数校验 ---

@pytest.mark.parametrize("max_chars", [0, 499, 100001, -1, "100", None, True, 1.5])
def test_fetch_rejects_bad_max_chars(monkeypatch, max_chars):
    fakes = FakeFetchers(monkeypatch)
    with pytest.raises(InvalidArgument):
        api.fetch(URL, max_chars=max_chars, settings=Settings())
    assert fakes.calls == []


@pytest.mark.parametrize("max_chars", [500, 8000, 100000])
def test_fetch_accepts_boundary_max_chars(monkeypatch, max_chars):
    FakeFetchers(monkeypatch)
    assert api.fetch(URL, max_chars=max_chars, settings=Settings()).url == URL


@pytest.mark.parametrize("url", ["", "file:///etc/passwd", "http://127.0.0.1/", "ftp://a.com/x"])
def test_fetch_rejects_bad_urls_before_fetching(monkeypatch, url):
    fakes = FakeFetchers(monkeypatch)
    with pytest.raises((InvalidArgument, BlockedUrl)):
        api.fetch(url, settings=Settings())
    assert fakes.calls == []


# --- fetch：顺序与兜底 ---

@pytest.mark.parametrize("order, expected_calls", [
    ("local_first", ["local"]),
    ("jina_first", ["jina"]),
    ("jina_only", ["jina"]),
    ("local_only", ["local"]),
])
def test_first_method_success_stops_there(monkeypatch, order, expected_calls):
    fakes = FakeFetchers(monkeypatch)
    page = api.fetch(URL, settings=Settings(fetch_order=order))
    assert fakes.calls == expected_calls
    assert page.method == expected_calls[0]


@pytest.mark.parametrize("order, expected_calls, method", [
    ("local_first", ["local", "jina"], "jina"),
    ("jina_first", ["jina", "local"], "local"),
])
def test_failure_falls_through_to_second_method(monkeypatch, order, expected_calls, method):
    first = expected_calls[0]
    fakes = FakeFetchers(monkeypatch, **{first: FetchFailed("坏了")})
    page = api.fetch(URL, settings=Settings(fetch_order=order))
    assert fakes.calls == expected_calls
    assert page.method == method
    assert page.title == f"{method}标题"


def test_timeout_also_falls_through(monkeypatch):
    fakes = FakeFetchers(monkeypatch, local=Timeout("超时"))
    assert api.fetch(URL, settings=Settings()).method == "jina"
    assert fakes.calls == ["local", "jina"]


def test_all_failures_are_summarised(monkeypatch):
    FakeFetchers(monkeypatch, local=FetchFailed("HTTP 521"), jina=Timeout("Jina 超时"))
    with pytest.raises(FetchFailed) as info:
        api.fetch(URL, settings=Settings())
    assert "local: HTTP 521" in info.value.message
    assert "jina: Jina 超时" in info.value.message


def test_local_only_never_touches_jina(monkeypatch):
    fakes = FakeFetchers(monkeypatch, local=FetchFailed("HTTP 521"))
    with pytest.raises(FetchFailed):
        api.fetch(URL, settings=Settings(fetch_order="local_only"))
    assert fakes.calls == ["local"]


@pytest.mark.parametrize("error", [BlockedUrl("重定向到内网"), InvalidArgument("坏参数")])
def test_blocked_or_invalid_never_falls_through(monkeypatch, error):
    fakes = FakeFetchers(monkeypatch, local=error)
    with pytest.raises(type(error)):
        api.fetch(URL, settings=Settings())
    assert fakes.calls == ["local"]


# --- fetch：结果与截断 ---

def test_page_fields(monkeypatch):
    FakeFetchers(monkeypatch, local=Fetched("https://example.com/final", "标题", CONTENT))
    page = api.fetch(URL, settings=Settings())
    assert isinstance(page, Page)
    assert (page.url, page.final_url, page.title) == (URL, "https://example.com/final", "标题")
    assert page.content == CONTENT
    assert page.chars == len(CONTENT)
    assert page.truncated is False
    assert page.elapsed_ms >= 0


def test_long_content_is_cut_at_paragraph_boundary(monkeypatch):
    paragraphs = [f"第{i}段" + "字" * 295 for i in range(10)]
    FakeFetchers(monkeypatch, local=Fetched(URL, "t", "\n\n".join(paragraphs)))
    page = api.fetch(URL, max_chars=1000, settings=Settings())
    assert page.truncated is True
    assert page.chars == len("\n\n".join(paragraphs))
    assert len(page.content) <= 1000
    assert page.content.endswith("字")
    assert page.content == "\n\n".join(paragraphs[:3])


def test_long_content_without_paragraphs_is_hard_cut(monkeypatch):
    FakeFetchers(monkeypatch, local=Fetched(URL, "t", "字" * 3000))
    page = api.fetch(URL, max_chars=1000, settings=Settings())
    assert page.truncated is True
    assert len(page.content) == 1000


def test_content_exactly_at_limit_is_not_truncated(monkeypatch):
    FakeFetchers(monkeypatch, local=Fetched(URL, "t", "字" * 1000))
    page = api.fetch(URL, max_chars=1000, settings=Settings())
    assert page.truncated is False
    assert len(page.content) == 1000


def test_default_max_chars_is_8000(monkeypatch):
    FakeFetchers(monkeypatch, local=Fetched(URL, "t", "字" * 9000))
    assert len(api.fetch(URL, settings=Settings()).content) == 8000


def test_settings_default_to_environment(monkeypatch, clean_env):
    clean_env.setenv("WEBSEARCH_FETCH_ORDER", "jina_only")
    fakes = FakeFetchers(monkeypatch)
    api.fetch(URL)
    assert fakes.calls == ["jina"]
