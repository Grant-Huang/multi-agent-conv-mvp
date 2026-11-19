"""GLiNER 适配器（可插拔，默认关闭）。

启用方式：在本服务虚拟环境中 `pip install gliner`（会引入 torch，故未列入锁文件），并设置
  DLP_ENABLE_GLINER=1
  DLP_GLINER_MODEL=<本地模型目录或 Hugging Face 模型名>
模型必须事先下载到本地；本服务不会在运行时自动联网下载（外发服务本身不应产生意外出网）。

GLiNER 只提供“候选实体”，不参与授权判决；低于阈值的结果丢弃，
与规则/主数据重叠时由 Detector 的合并优先级决定（GLiNER 优先级最低）。
"""
from __future__ import annotations

from typing import Any

from presidio_analyzer import EntityRecognizer, RecognizerResult

from .recognizers import LANG

# GLiNER 自然语言标签 → 本服务的规范实体类型
# 默认只用名称类标签：这些实体只会被替换（ANONYMIZE），不会让决策升级为审批 / 拦截。
# 机密信息与凭证的判定只由规则负责（D-019）：gold-v3 本地评测中，机密标签带来 23 次过度拦截，
# 只多拦住 1 条危险放行。需要时可通过 labels 参数显式加入 CONFIDENTIAL_LABEL。
DEFAULT_LABELS = {
    "customer company": "CUSTOMER",
    "supplier company": "CUSTOMER",
    "project code name": "PROJECT",
    "factory or plant": "PLANT",
    "person name": "PERSON",
}
CONFIDENTIAL_LABEL = {"confidential business information": "INTERNAL_INFO"}


class GlinerRecognizer(EntityRecognizer):
    def __init__(self, model: Any, labels: dict[str, str] | None = None, threshold: float = 0.5):
        """model：已加载的 GLiNER 模型对象（需提供 predict_entities(text, labels, threshold)）。"""
        self.model = model
        self.labels = labels or DEFAULT_LABELS
        self.threshold = threshold
        super().__init__(supported_entities=sorted(set(self.labels.values())), name="GlinerRecognizer",
                         supported_language=LANG)

    @classmethod
    def from_pretrained(cls, model_name: str, threshold: float = 0.5) -> "GlinerRecognizer":
        try:
            from gliner import GLiNER  # type: ignore[import-not-found]
        except ImportError as exc:  # 未安装时明确报错，由上层在启动阶段 fail closed
            raise RuntimeError("已启用 GLiNER，但未安装 gliner 包：请在本服务的虚拟环境中 pip install gliner") from exc
        # local_files_only：禁止运行时联网下载权重
        model = GLiNER.from_pretrained(model_name, local_files_only=True)
        return cls(model, threshold=threshold)

    def load(self) -> None:
        pass

    def analyze(self, text, entities, nlp_artifacts=None):
        preds = self.model.predict_entities(text, list(self.labels), threshold=self.threshold)
        out = []
        for p in preds:
            etype = self.labels.get(p["label"])
            if not etype or (entities and etype not in entities):
                continue
            out.append(RecognizerResult(etype, int(p["start"]), int(p["end"]), float(p.get("score", 0.5)),
                                        recognition_metadata={"recognizer_name": self.name, "source": "gliner"}))
        return out
