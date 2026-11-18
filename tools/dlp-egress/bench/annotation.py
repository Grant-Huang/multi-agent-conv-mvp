"""标注格式：解析 {TYPE:原文} 标记，并按默认策略推导期望决策与期望脱敏文本。

标记规则（详见 docs/ANNOTATION_GUIDE.md）：
- 实体写作 {TYPE:原文}，TYPE 必须是 ENTITY_TYPES 之一；不允许嵌套。
- 原文中的字面量花括号写作 {{ 和 }}。

期望结果按 default_policy.yaml 推导，与服务端策略保持一致：
- external_untrusted：SECRET→BLOCK、INTERNAL_INFO→REVIEW、METRIC/DEVICE_ID 保留、
  路由参数里的邮箱保留、AMOUNT 默认 Token 化（constraints 含 preserve:AMOUNT 时保留）、其余 Token 化
- internal：除 SECRET 外全部原样保留
- unknown（未登记目的地）：BLOCK，期望脱敏文本为空串（服务不返回任何载荷）
"""
from __future__ import annotations

import re
import unicodedata

# 供应商、合作方等外部组织统一标 CUSTOMER（服务端与 700 条评测集均如此）
ENTITY_TYPES = ("CUSTOMER", "PROJECT", "PLANT", "PERSON", "ORDER_ID", "DEVICE_ID", "EMAIL", "CONTACT",
                "AMOUNT", "METRIC", "INTERNAL_INFO", "SECRET")
TRUST_LEVELS = ("external_untrusted", "internal", "unknown")
ACTIONS = ("ALLOW", "ANONYMIZE", "REVIEW", "BLOCK")
_KEEP_EXTERNAL = {"METRIC", "DEVICE_ID"}
_ROUTING = re.compile(
    r"(?i)(?<![A-Za-z_])(?:to|cc|bcc|guests|recipients|attendees|recipient|reply_to)\s*[=:]\s*"
    r"(\[[^\]]*\]|\"[^\"]*\"|'[^']*')")


class AnnotationError(ValueError):
    """标注文本格式错误。消息直接展示给标注员，所以用中文写清楚位置和原因。"""


def parse_markup(marked: str) -> tuple[str, list[dict]]:
    """把带标记的文本解析为 (原文, 实体列表)。实体偏移按 Python Unicode 码点计数。"""
    out: list[str] = []
    entities: list[dict] = []
    length = 0  # 已输出原文的长度
    i, n = 0, len(marked)
    while i < n:
        ch = marked[i]
        if ch == "{" and marked.startswith("{{", i):
            out.append("{")
            length += 1
            i += 2
            continue
        if ch == "}" and marked.startswith("}}", i):
            out.append("}")
            length += 1
            i += 2
            continue
        if ch == "}":
            raise AnnotationError(f"第 {i + 1} 个字符处有未配对的 '}}'（字面量请写成 '}}}}'）")
        if ch == "{":
            m = re.match(r"\{([A-Za-z_]+):", marked[i:])
            if not m:
                raise AnnotationError(f"第 {i + 1} 个字符处的 '{{' 后应为 TYPE:（字面量请写成 '{{{{'）")
            etype = m.group(1)
            if etype not in ENTITY_TYPES:
                raise AnnotationError(f"未知实体类型 {etype}，可选：{'/'.join(ENTITY_TYPES)}")
            j = i + m.end()
            close = marked.find("}", j)
            if close < 0:
                raise AnnotationError(f"第 {i + 1} 个字符处的实体 {etype} 未闭合")
            text = marked[j:close]
            if "{" in text:
                raise AnnotationError(f"第 {i + 1} 个字符处的实体 {etype} 内不允许嵌套其他实体")
            if not text.strip():
                raise AnnotationError(f"第 {i + 1} 个字符处的实体 {etype} 的实体文本为空")
            if text != text.strip():
                raise AnnotationError(f"实体 {etype}「{text}」首尾不应包含空白")
            entities.append({"type": etype, "start": length, "end": length + len(text), "text": text})
            out.append(text)
            length += len(text)
            i = close + 1
            continue
        out.append(ch)
        length += 1
        i += 1
    return "".join(out), entities


def canonical(value: str) -> str:
    """与服务端 normalize.canonical 一致：去不可见字符、NFKC、合并空白、大小写折叠。"""
    value = "".join(c for c in value if unicodedata.category(c) != "Cf")
    return " ".join(unicodedata.normalize("NFKC", value).split()).casefold()


def derive_expected(src: str, entities: list[dict], trust: str, constraints: list[str]) -> tuple[str, str]:
    """按默认策略推导 (期望决策, 期望脱敏文本)。"""
    if trust not in TRUST_LEVELS:
        raise AnnotationError(f"destination_trust 必须是 {'/'.join(TRUST_LEVELS)} 之一")
    if trust == "unknown":
        return "BLOCK", ""
    routing = [m.span(1) for m in _ROUTING.finditer(src)]
    keep_amount = "preserve:AMOUNT" in constraints
    counters: dict[str, int] = {}
    tokens: dict[tuple[str, str], str] = {}
    rank = 0  # 0 ALLOW 1 ANONYMIZE 2 REVIEW 3 BLOCK
    pieces, pos = [], 0
    for e in sorted(entities, key=lambda x: x["start"]):
        pieces.append(src[pos:e["start"]])
        t, text = e["type"], e["text"]
        in_route = any(a <= e["start"] and e["end"] <= b for a, b in routing)
        if t == "SECRET":
            rep, rank = "[BLOCKED_SECRET]", max(rank, 3)
        elif trust == "internal":
            rep = text
        elif t == "INTERNAL_INFO":
            rep, rank = "[REDACTED_CONFIDENTIAL]", max(rank, 2)
        elif t in _KEEP_EXTERNAL or (t == "AMOUNT" and keep_amount) or (t == "EMAIL" and in_route):
            rep = text
        else:
            key = (t, canonical(text))
            if key not in tokens:
                counters[t] = counters.get(t, 0) + 1
                tokens[key] = f"[{t}_{counters[t]:03d}]"
            rep, rank = tokens[key], max(rank, 1)
        pieces.append(rep)
        pos = e["end"]
    pieces.append(src[pos:])
    return ACTIONS[rank], "".join(pieces)


def detect_language(text: str) -> str:
    return "zh" if re.search(r"[一-鿿]", text) else "en"
