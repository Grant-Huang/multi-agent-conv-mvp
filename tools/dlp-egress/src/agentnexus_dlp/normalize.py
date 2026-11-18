"""文本归一化：对抗全角字符、零宽字符等混淆，同时保留到原文的偏移映射。

识别在归一化文本上进行，命中的 span 再映射回原文偏移，最终替换作用在原文上，
因此不会把原文里的 "℃"、全角标点等“合法内容”顺带改掉（任务保真）。
"""
from __future__ import annotations

import unicodedata

# 零宽与不可见格式字符：常被用来拆开关键词绕过检测
_INVISIBLE = {
    "\u200b", "\u200c", "\u200d", "\u2060", "\ufeff", "\u00ad", "\u180e",
}


def normalize_with_map(text: str) -> tuple[str, list[int]]:
    """返回 (归一化文本, 归一化下标 → 原文下标 的映射表)。

    映射表长度 = 归一化文本长度 + 1，最后一个元素是原文长度，便于映射 span 的 end。
    """
    out_chars: list[str] = []
    index_map: list[int] = []
    for i, ch in enumerate(text):
        if ch in _INVISIBLE or unicodedata.category(ch) == "Cf":
            continue  # 删除不可见字符，不产生输出
        norm = unicodedata.normalize("NFKC", ch)
        for n in norm:
            out_chars.append(n)
            index_map.append(i)
    index_map.append(len(text))
    return "".join(out_chars), index_map


def map_span(index_map: list[int], start: int, end: int) -> tuple[int, int]:
    """把归一化文本上的 [start, end) 映射回原文 [start, end)。"""
    o_start = index_map[start]
    # end 指向归一化文本中 span 之后的位置；取 span 最后一个字符对应原文下标 + 1
    o_end = index_map[end - 1] + 1 if end > start else o_start
    return o_start, o_end


def canonical(value: str) -> str:
    """实体规范值：用于同一任务内“同一实体 → 同一 Token”的幂等判断。"""
    norm, _ = normalize_with_map(value)
    return " ".join(norm.split()).casefold()


def detect_language(text: str) -> str:
    """粗略语言判断：出现中日韩统一表意文字即视为 zh，否则 en。仅用于报告分桶。"""
    return "zh" if any("一" <= c <= "鿿" for c in text) else "en"
