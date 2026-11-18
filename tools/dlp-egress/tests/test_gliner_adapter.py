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
