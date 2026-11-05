"""访问真实网络的测试，默认跳过。运行：uv run pytest -m live -s"""
import pytest

from agent_websearch import api
from agent_websearch.errors import WebSearchError
from agent_websearch.settings import Settings

pytestmark = pytest.mark.live

QUERIES = [
    "llama.cpp kv-unified parallel slots",
    "Qwen3 模型 工具调用 最佳实践",
    "2026年 上海 人工智能 政策",
    "瀑布式开发 优缺点",
]
PAGES = [
    "https://www.gov.cn/",
    "https://blog.csdn.net/",
    "https://github.com/ggml-org/llama.cpp",
]


@pytest.mark.parametrize("query", QUERIES)
def test_search_returns_results(query):
    results = api.search(query, max_results=5)
    print(f"\n[{query}] {len(results)} 条")
    for r in results:
        print(f"  {r.rank}. {r.title[:40]}  {r.url}")
    assert results
    assert all(r.url.startswith("http") for r in results)


@pytest.mark.parametrize("order", ["local_first", "jina_first"])
@pytest.mark.parametrize("url", PAGES)
def test_fetch_real_pages(url, order):
    settings = Settings(fetch_order=order)
    try:
        page = api.fetch(url, max_chars=3000, settings=settings)
    except WebSearchError as exc:
        print(f"\n[{order}] {url} 失败：{exc.code} {exc.message}")
        pytest.skip(f"{url} 抓取失败（站点差异，仅记录）")
    print(f"\n[{order}] {url} -> {page.method} {page.chars} 字 {page.elapsed_ms}ms "
          f"《{page.title[:30]}》")
    assert page.content.strip()


def test_search_and_fetch_end_to_end():
    out = api.search_and_fetch("瀑布式开发 优缺点", max_results=3, max_chars_per_page=1500)
    print(f"\n页面 {len(out.pages)} 个，失败 {len(out.failures)} 个")
    for page in out.pages:
        print(f"  {page.method} {page.chars} 字 {page.url}")
    for failure in out.failures:
        print(f"  失败 {failure.url} {failure.code} {failure.message}")
    assert out.results
    assert out.pages or out.failures
