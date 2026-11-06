import sys
import types
from pathlib import Path

import pytest

from agent_websearch.errors import InvalidArgument
from agent_websearch.extract import html_to_markdown

BASE = "https://example.com/news/1"
FIXTURES = Path(__file__).parent / "fixtures"


def convert(html, extractor="lxml"):
    return html_to_markdown(html, BASE, extractor)


def body(inner):
    return f"<html><head><title>T</title></head><body>{inner}</body></html>"


@pytest.fixture(scope="module")
def article():
    html = (FIXTURES / "news_article.html").read_text(encoding="utf-8")
    return html_to_markdown(html, BASE, "lxml")


def test_article_title_comes_from_title_tag(article):
    assert article[0] == "瀑布模型详解 - 示例技术网"


def test_article_keeps_body_structure(article):
    md = article[1]
    assert "# 瀑布模型详解" in md
    assert "作者：张三" in md
    assert "**需求分析**" in md
    assert "## 主要阶段" in md
    assert "- 需求分析\n- 系统设计\n  - 架构设计\n  - 详细设计\n- 编码实现" in md
    assert "[官方文档](https://example.com/docs/waterfall)" in md
    assert "[参考站点](https://example.org/ref)" in md
    assert "```\nmake build\nmake test\n```" in md
    assert "| 阶段 | 产出 |\n| --- | --- |\n| 需求 | 需求规格说明 |" in md


@pytest.mark.parametrize("noise", [
    "首页", "新闻", "热门", "评论区", "广告", "版权所有", "tracking", "color: red",
])
def test_article_drops_navigation_ads_and_scripts(article, noise):
    assert noise not in article[1]


def test_blocks_are_separated_by_single_blank_line(article):
    assert "\n\n\n" not in article[1]
    assert article[1] == article[1].strip()


def test_title_falls_back_to_first_h1():
    html = "<html><body><h1>标题一</h1><p>正文</p></body></html>"
    title, _ = html_to_markdown(html, BASE, "lxml")
    assert title == "标题一"


def test_main_is_used_when_there_is_no_article():
    html = body("<div>侧边内容</div><main><p>" + "主体内容。" * 30 + "</p></main>")
    _, md = convert(html)
    assert "主体内容" in md
    assert "侧边内容" not in md


def test_largest_article_wins():
    html = body("<article><p>短</p></article><article><p>" + "长文章内容。" * 30 + "</p></article>")
    _, md = convert(html)
    assert "长文章内容" in md
    assert md.count("短") == 0


def test_paragraph_density_picks_content_container_without_semantic_tags():
    long_p = "<p>" + "这是正文段落，信息量很大。" * 10 + "</p>"
    html = body(
        '<div id="top"><p>顶部导航文字</p></div>'
        f'<div id="content">{long_p}{long_p}</div>'
        '<div id="rel"><p>相关推荐文字</p></div>'
    )
    _, md = convert(html)
    assert "这是正文段落" in md
    assert "顶部导航文字" not in md
    assert "相关推荐文字" not in md


def test_role_main_wrapper_keeps_all_nested_sections():
    # Sphinx 文档站：正文容器 role=main，正文被拆成多个 section，
    # 不能只选中"直接子 p 最多"的某一个小节
    def section(name, repeat):
        text = f"{name}的详细说明。" * repeat
        return f'<section id="{name}"><h2>{name}</h2><p>{text}</p></section>'

    html = body(
        '<div id="top"><p>顶部导航文字</p></div>'
        '<div class="body" role="main">'
        '<p>本章概述内容。</p>'
        + section("intro", 4) + section("details", 30) + section("summary", 5)
        + "</div>"
    )
    _, md = convert(html)
    assert "本章概述内容" in md
    assert "intro的详细说明" in md
    assert "details的详细说明" in md
    assert "summary的详细说明" in md
    assert "顶部导航文字" not in md


def test_itemprop_article_body_is_a_semantic_candidate():
    html = body(
        "<div><p>侧栏文字</p></div>"
        '<div itemprop="articleBody"><section><p>' + "文章主体内容。" * 20 + "</p></section>"
        "<section><p>" + "文章第二节内容。" * 20 + "</p></section></div>"
    )
    _, md = convert(html)
    assert "文章主体内容" in md
    assert "文章第二节内容" in md
    assert "侧栏文字" not in md


def test_tiny_page_still_returns_text():
    _, md = convert(body("<p>你好，世界</p>"))
    assert md == "你好，世界"


def test_wrapper_class_containing_sidebar_token_is_not_removed():
    html = body(
        '<div class="layout-with-sidebar"><article><p>' + "正文内容。" * 30 + "</p></article></div>"
    )
    _, md = convert(html)
    assert "正文内容" in md


def test_header_inside_article_is_kept_but_page_header_removed():
    html = body(
        "<header><p>站点页眉</p></header>"
        "<article><header><h1>文章标题</h1></header><p>" + "内容。" * 40 + "</p></article>"
        "<footer><p>站点页脚</p></footer>"
    )
    _, md = convert(html)
    assert "# 文章标题" in md
    assert "站点页眉" not in md
    assert "站点页脚" not in md


def test_hidden_elements_are_dropped():
    html = body(
        '<article><p>可见内容。</p><p hidden>隐藏一</p><p style="display: none">隐藏二</p>'
        "<p>" + "填充文字。" * 30 + "</p></article>"
    )
    _, md = convert(html)
    assert "可见内容" in md
    assert "隐藏" not in md


def test_form_controls_are_dropped_but_wrapping_form_is_kept():
    html = body(
        "<form><article><p>" + "表单里的正文。" * 30 + "</p></article>"
        '<input type="text" value="用户输入"><button>提交</button>'
        "<select><option>选项甲</option></select></form>"
    )
    _, md = convert(html)
    assert "表单里的正文" in md
    for noise in ("用户输入", "提交", "选项甲"):
        assert noise not in md


def test_ordered_list_and_inline_markup():
    html = body(
        "<article><ol><li>第一步</li><li>第二步 <em>注意</em> <code>x=1</code></li></ol>"
        "<p>" + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert "1. 第一步\n2. 第二步 *注意* `x=1`" in md


def test_blockquote_and_line_break():
    html = body(
        "<article><blockquote>引用的话</blockquote><p>第一行<br>第二行</p>"
        "<p>" + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert "> 引用的话" in md
    assert "第一行\n第二行" in md


@pytest.mark.parametrize("href, expected", [
    ("/a/b", "[链接](https://example.com/a/b)"),
    ("c.html", "[链接](https://example.com/news/c.html)"),
    ("//cdn.example.org/x", "[链接](https://cdn.example.org/x)"),
    ("#top", "链接"),
    ("javascript:void(0)", "链接"),
    ("mailto:a@b.com", "链接"),
])
def test_link_resolution(href, expected):
    html = body(
        f'<article><p><a href="{href}">链接</a></p><p>' + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert expected in md
    if not expected.startswith("["):
        assert "](" not in md


def test_image_only_link_and_images_are_dropped():
    html = body(
        '<article><p><a href="/x"><img src="a.png" alt="图"></a>文字</p><p>'
        + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert "文字" in md
    assert "a.png" not in md
    assert "[](" not in md


def test_pipe_in_table_cell_is_escaped():
    html = body(
        "<article><table><tr><th>a</th><th>b</th></tr><tr><td>x|y</td><td>z</td></tr></table>"
        "<p>" + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert "| x\\|y | z |" in md


def test_table_without_header_row_gets_one_and_short_rows_are_padded():
    html = body(
        "<article><table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>"
        "<p>" + "填充。" * 40 + "</p></article>"
    )
    _, md = convert(html)
    assert "| a | b |\n| --- | --- |\n| c |  |" in md


def test_layout_table_is_treated_as_plain_content():
    html = body(
        "<table><tr><td><p>" + "左栏正文段落。" * 20 + "</p></td>"
        "<td><p>右栏正文段落。" + "更多。" * 20 + "</p></td></tr></table>"
    )
    _, md = convert(html)
    assert "左栏正文段落" in md and "右栏正文段落" in md
    assert "---" not in md


def test_entities_are_decoded():
    _, md = convert(body("<p>a &amp; b &lt;c&gt; &nbsp;d</p>"))
    assert "a & b <c>" in md


@pytest.mark.parametrize("html", ["", "   ", "<html></html>", "<script>x</script>"])
def test_empty_documents_give_empty_result(html):
    assert convert(html) == ("", "")


# --- extractor 选择 ---

def fake_trafilatura(monkeypatch, result):
    calls = {}
    module = types.ModuleType("trafilatura")

    def extract(html, **kwargs):
        calls["kwargs"] = kwargs
        return result

    module.extract = extract
    monkeypatch.setitem(sys.modules, "trafilatura", module)
    return calls


def test_auto_prefers_trafilatura_when_installed(monkeypatch):
    calls = fake_trafilatura(monkeypatch, "# 来自 trafilatura")
    title, md = html_to_markdown(body("<p>lxml 内容</p>"), BASE, "auto")
    assert md == "# 来自 trafilatura"
    assert title == "T"
    assert calls["kwargs"]["output_format"] == "markdown"
    assert calls["kwargs"]["url"] == BASE


def test_auto_falls_back_to_lxml_when_trafilatura_returns_nothing(monkeypatch):
    fake_trafilatura(monkeypatch, None)
    _, md = html_to_markdown(body("<p>lxml 内容</p>"), BASE, "auto")
    assert md == "lxml 内容"


def test_auto_uses_lxml_when_trafilatura_missing(monkeypatch):
    monkeypatch.setitem(sys.modules, "trafilatura", None)
    _, md = html_to_markdown(body("<p>lxml 内容</p>"), BASE, "auto")
    assert md == "lxml 内容"


def test_lxml_mode_never_touches_trafilatura(monkeypatch):
    calls = fake_trafilatura(monkeypatch, "不该用到")
    _, md = html_to_markdown(body("<p>lxml 内容</p>"), BASE, "lxml")
    assert md == "lxml 内容"
    assert "kwargs" not in calls


def test_forcing_trafilatura_without_install_is_an_error(monkeypatch):
    monkeypatch.setitem(sys.modules, "trafilatura", None)
    with pytest.raises(InvalidArgument):
        html_to_markdown(body("<p>x</p>"), BASE, "trafilatura")


def test_comment_inside_removed_script_does_not_crash():
    html = body("<script><!-- tracker --></script><p>正文还在。</p>")
    _, md = convert(html)
    assert "正文还在" in md
    assert "tracker" not in md


def test_unknown_extractor_is_an_error():
    with pytest.raises(InvalidArgument):
        html_to_markdown(body("<p>x</p>"), BASE, "bs4")
