"""GLiNER 适配器测试：用假模型验证接入路径（真实模型在当前环境无法下载，见 README）。"""
import pytest

from agentnexus_dlp.detector import Detector
from agentnexus_dlp.gliner_adapter import GlinerRecognizer
from bench.fake_secrets import STRIPE_LIVE  # noqa: E402


class FakeGliner:
    """模拟 GLiNER.predict_entities 的返回格式。"""

    def predict_entities(self, text, labels, threshold=0.5):
        out = []
        for name, label in (("蓝鲸", "customer company"), ("王晓峰", "person name")):
            i = text.find(name)
            if i >= 0:
                out.append({"start": i, "end": i + len(name), "label": label, "score": 0.8, "text": name})
        return out


def test_gliner_candidates_are_merged_into_detection():
    det = Detector(master_data=None, gliner_recognizer=GlinerRecognizer(FakeGliner()))
    got = {(e.type, e.source) for e in det.detect("请把报告发给王晓峰，他负责蓝鲸对接。", "zh")}
    assert ("PERSON", "gliner") in got
    assert ("CUSTOMER", "gliner") in got


def test_rules_win_over_gliner_on_overlap():
    class Overlapping(FakeGliner):
        def predict_entities(self, text, labels, threshold=0.5):
            return [{"start": 0, "end": len(text), "label": "person name", "score": 0.99}]
    det = Detector(master_data=None, gliner_recognizer=GlinerRecognizer(Overlapping()))
    ents = det.detect(f"Authorization: Bearer {STRIPE_LIVE}", "en")
    assert [e.type for e in ents] == ["SECRET"]


def test_enabled_without_model_fails_at_startup():
    with pytest.raises(RuntimeError):
        Detector(master_data=None, enable_gliner=True, gliner_model=None)


def test_benchmark_can_enable_gliner(monkeypatch, tmp_path):
    """本地测试指南依赖：评测脚本可通过 gliner_model 开启 GLiNER，并在报告中记录配置。"""
    from bench.holdout import holdout_rows
    from bench.run_benchmark import run_benchmark

    seen = {}

    def fake_from_pretrained(cls, model_name, threshold=0.5):
        seen.update(model=model_name, threshold=threshold)
        return cls(FakeGliner(), threshold=threshold)

    monkeypatch.setattr(GlinerRecognizer, "from_pretrained", classmethod(fake_from_pretrained))
    rep = run_benchmark(None, "B_no_dictionary", tmp_path, rows=holdout_rows(),
                        gliner_model="/models/fake", gliner_threshold=0.3)
    assert seen == {"model": "/models/fake", "threshold": 0.3}
    assert rep["config"]["gliner_model"] == "/models/fake"
    assert rep["config"]["gliner_threshold"] == 0.3
    assert rep["safety"]["secret_missed"] == 0


# ---------------------------------------------------------------- GLiNER 不参与“升级决策”（D-019）
class LabelEchoGliner:
    """只对调用方实际请求的标签返回结果，用来检查适配器向模型请求了哪些标签。"""

    def __init__(self):
        self.requested = []

    def predict_entities(self, text, labels, threshold=0.5):
        self.requested = list(labels)
        out = []
        if "confidential business information" in labels:
            out.append({"start": 0, "end": 4, "label": "confidential business information", "score": 0.9})
        if "person name" in labels and "王晓峰" in text:
            i = text.find("王晓峰")
            out.append({"start": i, "end": i + 3, "label": "person name", "score": 0.9})
        return out


def test_default_gliner_labels_only_cover_name_like_entities():
    """默认标签只含名称类实体（客户、项目、厂区、人名），不含会触发审批 / 拦截的类型。

    gold-v3 本地评测：机密标签带来 23 次过度拦截，只多拦住 1 条危险放行。
    """
    model = LabelEchoGliner()
    det = Detector(master_data=None, gliner_recognizer=GlinerRecognizer(model))
    got = {(e.type, e.source) for e in det.detect("年度计划请发给王晓峰确认", "zh")}
    assert "confidential business information" not in model.requested
    assert ("PERSON", "gliner") in got
    assert not any(t in ("INTERNAL_INFO", "SECRET") for t, _ in got)


def test_confidential_label_can_still_be_enabled_explicitly():
    labels = {"person name": "PERSON", "confidential business information": "INTERNAL_INFO"}
    det = Detector(master_data=None, gliner_recognizer=GlinerRecognizer(LabelEchoGliner(), labels=labels))
    got = {(e.type, e.source) for e in det.detect("年度计划请发给王晓峰确认", "zh")}
    assert ("INTERNAL_INFO", "gliner") in got
