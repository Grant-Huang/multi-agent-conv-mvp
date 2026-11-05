"""HTML 正文提取并转成 Markdown。

`auto`：装了 trafilatura 就用它（质量更好），否则用内置的 lxml 实现。
内置实现的目标是"够用"：去掉导航、广告、脚本，选出正文容器，转成常用的 Markdown 结构。
"""
import re
from urllib.parse import urljoin

from lxml import etree, html as lxml_html

from .errors import InvalidArgument

MIN_CANDIDATE_CHARS = 80
_JUNK_TAGS = ("script", "style", "noscript", "iframe", "svg", "canvas", "template",
              "nav", "aside", "input", "button", "select", "textarea", "img", "picture")
_JUNK_TOKENS = {
    "nav", "navbar", "navigation", "menu", "sidebar", "breadcrumb", "breadcrumbs",
    "ad", "ads", "advert", "advertisement", "banner", "comment", "comments", "cookie",
    "popup", "modal", "share", "social", "related", "recommend", "footer", "copyright",
}
_PROTECTED_TAGS = {"html", "body", "article", "main"}
_BLOCK_TAGS = {
    "p", "div", "section", "article", "main", "header", "footer", "figure", "figcaption",
    "dl", "dt", "dd", "address", "details", "summary", "body", "html", "center",
    "tr", "td", "th", "tbody", "thead", "tfoot", "table", "form", "fieldset",
}
_HEADINGS = {f"h{i}": "#" * i + " " for i in range(1, 7)}
_NO_LINK_PREFIXES = ("#", "javascript:", "mailto:", "tel:", "data:")
_LAYOUT_XPATH = ".//table | .//div | .//p | .//ul | .//ol | .//h1 | .//h2 | .//h3"
_TOKEN_SPLIT = re.compile(r"[\s_\-]+")
_HIDDEN_STYLE = re.compile(r"display\s*:\s*none|visibility\s*:\s*hidden", re.I)


def html_to_markdown(html: str, base_url: str, extractor: str = "auto") -> tuple[str, str]:
    """返回 (标题, Markdown 正文)；提取不到内容时返回 ("", "")。"""
    if extractor not in ("auto", "lxml", "trafilatura"):
        raise InvalidArgument(f"未知的正文提取方式：{extractor}")
    root = _parse(html)
    title = _title(root) if root is not None else ""
    if extractor != "lxml":
        markdown = _with_trafilatura(html, base_url, required=extractor == "trafilatura")
        if markdown:
            return title, markdown
    if root is None:
        return "", ""
    return title, _lxml_markdown(root, base_url)


def _with_trafilatura(html: str, base_url: str, required: bool) -> str | None:
    try:
        import trafilatura
    except ImportError:
        if required:
            raise InvalidArgument(
                "未安装 trafilatura，请安装 agent-websearch[extract]"
            ) from None
        return None
    result = trafilatura.extract(
        html, url=base_url, output_format="markdown",
        include_links=True, include_tables=True, favor_recall=True,
    )
    return result.strip() if result else None


def _parse(html: str):
    if not html or not html.strip():
        return None
    try:
        return lxml_html.fromstring(html)
    except (etree.ParserError, etree.XMLSyntaxError, ValueError):
        return None


def _squash(text: str) -> str:
    return re.sub(r"\s+", " ", text)


def _title(root) -> str:
    found = root.xpath("//title")
    if found and found[0].text_content().strip():
        return _squash(found[0].text_content()).strip()
    h1 = root.xpath("//h1")
    return _squash(h1[0].text_content()).strip() if h1 else ""


# ---------- 清理与选根 ----------

def _clean(root) -> None:
    for el in list(root.iter(*_JUNK_TAGS)):
        el.drop_tree()
    for el in list(root.iter()):
        if not isinstance(el.tag, str) or el.getparent() is None and el is root:
            continue
        if _is_hidden(el) or _is_junk_by_class(el) or _is_page_chrome(el):
            el.drop_tree()
    for comment in list(root.xpath("//comment()")):
        comment.drop_tree()


def _is_hidden(el) -> bool:
    return "hidden" in el.attrib or bool(_HIDDEN_STYLE.search(el.get("style", "")))


def _is_junk_by_class(el) -> bool:
    if el.tag in _PROTECTED_TAGS:
        return False
    tokens = _TOKEN_SPLIT.split(f"{el.get('class', '')} {el.get('id', '')}".lower())
    if not _JUNK_TOKENS.intersection(tokens):
        return False
    return not el.xpath(".//article | .//main")


def _is_page_chrome(el) -> bool:
    """页面级的 header/footer（不在 article/main 里）是页眉页脚，文章自带的 header 要保留。"""
    return el.tag in ("header", "footer") and not el.xpath("ancestor::article | ancestor::main")


def _text_len(el) -> int:
    return len(_squash(el.text_content()).strip())


def _pick_roots(root) -> list:
    """选出正文容器（文档顺序）。多栏布局里密度相近的兄弟容器也算正文。"""
    candidates = root.xpath(
        "//article | //main | //*[@role='main'] | //*[@itemprop='articleBody']"
    )
    if candidates:
        best = max(candidates, key=_text_len)
        if _text_len(best) >= MIN_CANDIDATE_CHARS:
            return [best]
    containers = root.xpath("//body | //div | //section | //td")
    if not containers:
        return [root]

    def density(el) -> int:
        return sum(len(_squash(p.text_content()).strip()) for p in el.xpath("./p"))

    best = max(containers, key=density)
    best_density = density(best)
    if best_density == 0:
        return [root]
    parent = best.getparent()
    if parent is None:
        return [best]
    return [
        c for c in parent
        if c is best or (c in containers and density(c) * 3 >= best_density)
    ]


# ---------- 转换 ----------

def _lxml_markdown(root, base_url: str) -> str:
    _clean(root)
    writer = _Writer(base_url)
    for chosen in _pick_roots(root):
        writer.walk(chosen)
    writer.flush()
    text = "\n\n".join(writer.blocks)
    return re.sub(r"\n{3,}", "\n\n", text).strip()


def _inline(el, base_url: str) -> str:
    """把元素按行内内容转成文本（链接、粗体、斜体、代码、换行）。"""
    tag = el.tag
    inner = _inline_children(el, base_url)
    if tag == "br":
        return "\n"
    if tag == "a":
        text = inner.strip()
        href = (el.get("href") or "").strip()
        if not text:
            return ""
        if not href or href.lower().startswith(_NO_LINK_PREFIXES):
            return text
        return f"[{text}]({urljoin(base_url, href)})"
    if tag in ("strong", "b") and inner.strip():
        return f"**{inner.strip()}**"
    if tag in ("em", "i") and inner.strip():
        return f"*{inner.strip()}*"
    if tag == "code" and inner.strip():
        return f"`{inner.strip()}`"
    return inner


def _inline_children(el, base_url: str) -> str:
    parts = [_squash(el.text or "")]
    for child in el:
        if isinstance(child.tag, str):
            parts.append(_inline(child, base_url))
        parts.append(_squash(child.tail or ""))
    return "".join(parts)


def _tidy(text: str) -> str:
    text = re.sub(r" *\n *", "\n", text)
    return re.sub(r" {2,}", " ", text).strip()


class _Writer:
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.blocks: list[str] = []
        self.buf: list[str] = []

    def add(self, text: str) -> None:
        self.buf.append(_squash(text))

    def flush(self, prefix: str = "") -> None:
        text = _tidy("".join(self.buf))
        self.buf = []
        if not text:
            return
        if prefix == "> ":
            text = text.replace("\n", "\n" + prefix)
        self.blocks.append(prefix + text)

    def walk(self, el) -> None:
        tag = el.tag
        if not isinstance(tag, str):
            return
        if tag in _HEADINGS:
            self.flush()
            self.buf.append(_inline_children(el, self.base_url))
            self.flush(_HEADINGS[tag])
        elif tag in ("ul", "ol"):
            self.flush()
            lines = self._list(el, 0)
            if lines:
                self.blocks.append("\n".join(lines))
        elif tag == "pre":
            self.flush()
            code = el.text_content().strip("\n")
            if code.strip():
                self.blocks.append(f"```\n{code}\n```")
        elif tag == "table" and not el.xpath(_LAYOUT_XPATH):
            self.flush()
            table = self._table(el)
            if table:
                self.blocks.append(table)
        elif tag == "blockquote":
            self.flush()
            self.buf.append(_inline_children(el, self.base_url))
            self.flush("> ")
        elif tag in ("a", "strong", "b", "em", "i", "code", "br"):
            self.buf.append(_inline(el, self.base_url))
        elif tag in _BLOCK_TAGS:
            self.flush()
            self._children(el)
            self.flush()
        else:
            self._children(el)

    def _children(self, el) -> None:
        if el.text:
            self.add(el.text)
        for child in el:
            self.walk(child)
            if child.tail:
                self.add(child.tail)

    def _list(self, el, depth: int) -> list[str]:
        lines = []
        for index, li in enumerate((c for c in el if c.tag == "li"), start=1):
            marker = f"{index}." if el.tag == "ol" else "-"
            parts = [_squash(li.text or "")]
            nested = []
            for child in li:
                if child.tag in ("ul", "ol"):
                    nested.extend(self._list(child, depth + 1))
                elif isinstance(child.tag, str):
                    parts.append(_inline(child, self.base_url))
                parts.append(_squash(child.tail or ""))
            text = _tidy("".join(parts)).replace("\n", " ")
            if text:
                lines.append(f"{'  ' * depth}{marker} {text}")
            lines.extend(nested)
        return lines

    def _table(self, el) -> str:
        rows = []
        for tr in el.iter("tr"):
            cells = [
                _tidy(_inline_children(cell, self.base_url)).replace("\n", " ").replace("|", "\\|")
                for cell in tr if cell.tag in ("td", "th")
            ]
            if cells:
                rows.append(cells)
        if not rows:
            return ""
        width = max(len(r) for r in rows)
        rows = [r + [""] * (width - len(r)) for r in rows]
        lines = [_row(rows[0]), _row(["---"] * width)]
        lines.extend(_row(r) for r in rows[1:])
        return "\n".join(lines)


def _row(cells: list[str]) -> str:
    return "| " + " | ".join(cells) + " |"
