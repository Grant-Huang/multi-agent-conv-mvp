"""识别层编排：Presidio AnalyzerEngine + 自定义识别器 + detect-secrets + 可选 GLiNER。

流程：归一化（NFKC、去零宽字符）→ Presidio 调用全部识别器 → 跨类型 span 合并 → 映射回原文偏移。

关于 NLP 引擎：Presidio 默认要加载 spaCy 的 en_core_web_lg 等大模型。本服务的识别全部基于
规则、词典、detect-secrets 与可选 GLiNER，不依赖 spaCy 的 NER，所以这里用 spacy.blank("xx")
的多语言分词器实现一个轻量 NlpEngine，避免为外发安全服务引入一个需要联网下载的模型。
"""
from __future__ import annotations

import logging
from typing import Iterable

import spacy
from presidio_analyzer import AnalyzerEngine, RecognizerRegistry
from presidio_analyzer.nlp_engine import NlpArtifacts, NlpEngine

from .models import DetectedEntity
from .normalize import map_span, normalize_with_map
from .recognizers import LANG, MasterDataRecognizer, builtin_recognizers
from .secrets import DetectSecretsRecognizer

log = logging.getLogger(__name__)

# 合并优先级：数值越大越优先。凭证最高；租户主数据（精确名单）高于启发式规则；GLiNER 最低。
SOURCE_PRIORITY = {"detect_secrets": 100, "master_data": 70, "rule": 40, "gliner": 30}
TYPE_PRIORITY = {
    "SECRET": 100, "INTERNAL_INFO": 80, "EMAIL": 60, "CONTACT": 60, "ORDER_ID": 55, "DEVICE_ID": 55,
    "AMOUNT": 50, "METRIC": 45, "CUSTOMER": 40, "PROJECT": 40, "PLANT": 40, "PERSON": 40,
}


class BlankNlpEngine(NlpEngine):
    """只做分词的轻量 NLP 引擎（不含 NER / 词形还原模型）。"""

    def __init__(self):
        self._nlp = None

    def load(self) -> None:
        self._nlp = spacy.blank("xx")

    def is_loaded(self) -> bool:
        return self._nlp is not None

    def process_text(self, text: str, language: str) -> NlpArtifacts:
        doc = self._nlp(text)
        return NlpArtifacts(entities=[], tokens=doc, tokens_indices=[t.idx for t in doc],
                            lemmas=[t.text.lower() for t in doc], nlp_engine=self, language=language)

    def process_batch(self, texts: Iterable[str], language: str, batch_size: int = 1, n_process: int = 1, **kwargs):
        for text in texts:
            yield text, self.process_text(text, language)

    def is_stopword(self, word: str, language: str) -> bool:
        return False

    def is_punct(self, word: str, language: str) -> bool:
        return bool(word) and all(not c.isalnum() for c in word)

    def get_supported_entities(self) -> list[str]:
        return []

    def get_supported_languages(self) -> list[str]:
        return [LANG]


class Detector:
    def __init__(self, master_data: dict[str, list[str]] | None = None, enable_gliner: bool = False,
                 gliner_model: str | None = None, gliner_recognizer=None, gliner_threshold: float = 0.5):
        registry = RecognizerRegistry(supported_languages=[LANG])
        for rec in builtin_recognizers():
            registry.add_recognizer(rec)
        registry.add_recognizer(DetectSecretsRecognizer())
        if master_data:
            registry.add_recognizer(MasterDataRecognizer(master_data))
        if gliner_recognizer is not None:
            registry.add_recognizer(gliner_recognizer)
        elif enable_gliner:
            # 启用但无法加载时直接抛错：启动失败好过“以为在用模型，其实没用”
            from .gliner_adapter import GlinerRecognizer
            if not gliner_model:
                raise RuntimeError("已启用 GLiNER，但未配置 DLP_GLINER_MODEL")
            registry.add_recognizer(GlinerRecognizer.from_pretrained(gliner_model, threshold=gliner_threshold))
        nlp = BlankNlpEngine()
        nlp.load()
        self.engine = AnalyzerEngine(registry=registry, nlp_engine=nlp, supported_languages=[LANG])
        self.recognizer_names = [r.name for r in registry.recognizers]

    def detect(self, text: str, language: str | None = None) -> list[DetectedEntity]:
        """识别 text 中的实体，返回按原文偏移排序、互不重叠的实体列表。

        language 只用于报告，识别始终使用全部（中英）规则，以覆盖中英混排。
        """
        if not text:
            return []
        norm, index_map = normalize_with_map(text)
        raw = self.engine.analyze(text=norm, language=LANG, score_threshold=0.0)
        candidates = []
        for r in raw:
            meta = r.recognition_metadata or {}
            source = meta.get("source", "rule")
            candidates.append((r.entity_type, r.start, r.end, r.score, source))
        merged = merge_spans(candidates)
        out = []
        for etype, start, end, score, source in merged:
            o_start, o_end = map_span(index_map, start, end)
            out.append(DetectedEntity(type=etype, start=o_start, end=o_end, score=round(score, 3), source=source))
        return out


def merge_spans(candidates):
    """跨识别器合并重叠 span：按 (类型优先级, 来源优先级, 长度, 分数) 贪心选取，不允许交叉。

    同一位置被凭证规则和其他规则同时命中时，凭证优先，保证不会因为被识别成别的类型而外发。
    """
    def key(c):
        etype, start, end, score, source = c
        return (-TYPE_PRIORITY.get(etype, 0), -SOURCE_PRIORITY.get(source, 0), -(end - start), -score, start)

    chosen: list[tuple] = []
    for c in sorted(candidates, key=key):
        _, s, e, _, _ = c
        if any(s < ce and cs < e for _, cs, ce, _, _ in chosen):
            continue
        chosen.append(c)
    return sorted(chosen, key=lambda c: c[1])
