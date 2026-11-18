"""识别层测试：每类实体至少一条正例；区分工单号与设备号；凭证必须被抓到。"""
import pytest

from agentnexus_dlp.detector import Detector
from bench.fake_secrets import (AWS_DOC_EXAMPLE, AWS_KEY_ID_KEY, GITHUB_A, GITHUB_B, GITHUB_C, PEM_BEGIN, PEM_END,
                                STRIPE_LIVE)


@pytest.fixture(scope="module")
def detector():
    # 不带主数据词典：只测通用规则与 detect-secrets 的泛化能力
    return Detector(master_data=None)


def found(detector, text, lang="zh"):
    """返回 {(类型, 原文片段)}，方便断言。"""
    return {(e.type, text[e.start:e.end]) for e in detector.detect(text, lang)}


def test_bearer_token_is_secret(detector):
    text = f"调用参数 Authorization: Bearer {STRIPE_LIVE} 发往供应商"
    assert ("SECRET", STRIPE_LIVE) in found(detector, text)


def test_detect_secrets_plugin_catches_aws_key(detector):
    text = f"config: {AWS_KEY_ID_KEY} = {AWS_DOC_EXAMPLE}"
    types = {t for t, _ in found(detector, text, "en")}
    assert "SECRET" in types


def test_private_key_block_is_secret(detector):
    text = f"{PEM_BEGIN}\nMIIEow\n{PEM_END}"
    assert any(t == "SECRET" for t, _ in found(detector, text, "en"))


def test_email_and_phones(detector):
    text = "联系人 buyer@acme-demo.com，电话13612345678，美国 +1-415-555-0199"
    got = found(detector, text)
    assert ("EMAIL", "buyer@acme-demo.com") in got
    assert ("CONTACT", "13612345678") in got
    assert ("CONTACT", "+1-415-555-0199") in got


def test_order_id_vs_device_id(detector):
    text = "设备AGV-03停机，关联工单WO-559120和批次LOT-88A"
    got = found(detector, text)
    assert ("DEVICE_ID", "AGV-03") in got
    assert ("ORDER_ID", "WO-559120") in got
    assert ("ORDER_ID", "LOT-88A") in got


def test_amounts_and_metrics(detector):
    got = found(detector, "合同金额56万元，良率97.5%，温度210℃")
    assert ("AMOUNT", "56万元") in got
    assert ("METRIC", "良率97.5%") in got
    assert ("METRIC", "温度210℃") in got
    got_en = found(detector, "budget EUR 12,500 with cycle time 37 s", "en")
    assert ("AMOUNT", "EUR 12,500") in got_en
    assert ("METRIC", "cycle time 37 s") in got_en


def test_confidential_business_terms(detector):
    got = found(detector, "附件里有内部成本结构，请勿外传")
    assert any(t == "INTERNAL_INFO" for t, _ in got)
    got_en = found(detector, "This includes our confidential margin targets.", "en")
    assert any(t == "INTERNAL_INFO" for t, _ in got_en)


def test_generic_chinese_company_project_person(detector):
    got = found(detector, "请把鹤汀精工的项目松涛进展发给许经理")
    assert ("CUSTOMER", "鹤汀精工") in got
    assert ("PROJECT", "项目松涛") in got
    assert ("PERSON", "许经理") in got


def test_master_data_dictionary_is_used():
    det = Detector(master_data={"CUSTOMER": ["某某特殊名称"]})
    got = {(e.type, e.source) for e in det.detect("客户某某特殊名称来访", "zh")}
    assert ("CUSTOMER", "master_data") in got


def test_clean_text_has_no_entities(detector):
    assert found(detector, "设备保养通用检查表包括清洁、润滑、紧固和记录。") == set()


def test_overlapping_spans_are_merged(detector):
    # 同一位置被多个识别器命中时，只保留一个，且不产生交叉 span
    ents = detector.detect(f"Authorization: Bearer {GITHUB_A}", "en")
    spans = sorted((e.start, e.end) for e in ents)
    for (s1, e1), (s2, e2) in zip(spans, spans[1:]):
        assert e1 <= s2


@pytest.mark.parametrize("text,value", [
    # 回归：留出集 HO-020 曾漏放。detect-secrets 的 GitHub 插件只返回 "ghp" 前缀，旧实现把它当作过短结果丢弃
    (f"token: {GITHUB_B}", GITHUB_B),
    (f"见日志 {GITHUB_C} 已提交", GITHUB_C),
])
def test_prefix_only_plugin_results_cover_full_token(detector, text, value):
    assert ("SECRET", value) in found(detector, text)
