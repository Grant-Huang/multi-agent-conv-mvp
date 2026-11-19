"""识别规则第二轮改进（依据 1000 条开发集的失败类别）。

例子全部为新编写，不取自开发集，用来约束规则的通用性。
"""
import pytest

from agentnexus_dlp.detector import Detector


@pytest.fixture(scope="module")
def det():
    return Detector(master_data=None)


def found(det, text):
    return {(e.type, text[e.start:e.end]) for e in det.detect(text, None)}


# ---------------------------------------------------------------- 凭证
# 合成值在运行时拼接，避免源码中出现完整「key=value」形态触发 gitleaks（见 D-007）。
def _sec(*parts: str) -> str:
    return "-".join(parts)


_SECRET_CASES = [
    (f"db_password: {_sec('Rv8', 'Lumen', 'Quartz')}", _sec("Rv8", "Lumen", "Quartz")),
    (f"client_secret = {_sec('Oak', 'Delta', '552190')}", _sec("Oak", "Delta", "552190")),
    (f"signing_key_material={_sec('Kestrel', '0429', 'Moss')}", _sec("Kestrel", "0429", "Moss")),
    (f"service_credential_ref: {_sec('Ivory', '7731', 'Fern')}", _sec("Ivory", "7731", "Fern")),
    ("密码：" + "Abc" + "#" + "12345xyz", "Abc" + "#" + "12345xyz"),
    ("口令是 " + _sec("Zeta", "7788", "Pass"), _sec("Zeta", "7788", "Pass")),
    ("访问令牌：" + "tk" + "-" + "9a8b7c6d5e", "tk" + "-" + "9a8b7c6d5e"),
    (f"数据库密钥={_sec('Velvet', '3301', 'Pine')}", _sec("Velvet", "3301", "Pine")),
]


@pytest.mark.parametrize("text, value", _SECRET_CASES)
def test_secret_key_variants(det, text, value):
    assert ("SECRET", value) in found(det, text)


def test_secret_value_is_masked_everywhere_in_the_text(det):
    value = _sec("Kappa", "2291", "Xyzzy")
    text = f"auth_token={value}；稍后的备注又提到 {value}，请核对"
    hits = [e for e in det.detect(text, None) if e.type == "SECRET"]
    assert len(hits) == 2


def test_secret_is_not_split_into_an_order_id(det):
    value = _sec("ALPHA", "BRAVO", "CHARLIE", "9911")
    text = f"secret_value={value}，另见 {value}"
    got = found(det, text)
    assert ("SECRET", value) in got
    assert not any(t == "ORDER_ID" for t, _ in got)


@pytest.mark.parametrize("text", [
    "token 数量为 3",            # “token”后不是赋值
    "password reset 指南已发布",  # 没有值
    "key = 3",                   # 值过短
])
def test_secret_negatives(det, text):
    assert not any(t == "SECRET" for t, _ in found(det, text))


# ---------------------------------------------------------------- 机密信息
@pytest.mark.parametrize("text, span", [
    ("附件含未公开的供应商返利阈值，请勿外传", "未公开的供应商返利阈值"),
    ("这是内部定价模型", "内部定价模型"),
    ("请勿转发尚未公开的产能扩张计划", "尚未公开的产能扩张计划"),
    ("Attached is the confidential supplier scorecard.", "confidential supplier scorecard"),
    ("Do not share the restricted pricing playbook.", "restricted pricing playbook"),
    ("It mentions our nonpublic rebate matrix.", "nonpublic rebate matrix"),
    ("参考内部报价底线第3版", "内部报价底线第3版"),
    ("See the internal price floor rev 2 for details.", "internal price floor rev 2"),
    ("附上限制传播的产线改造计划", "限制传播的产线改造计划"),
    ("这是仅限内部的价格规则", "仅限内部的价格规则"),
    ("请勿转发严禁外传的设备参数", "严禁外传的设备参数"),
])
def test_confidential_modifier_phrases(det, text, span):
    assert ("INTERNAL_INFO", span) in found(det, text)


@pytest.mark.parametrize("text", [
    "下周举行内部培训，请准时参加",
    "The internal meeting starts at 10.",
    "公开的产品手册已更新",
])
def test_confidential_negatives(det, text):
    assert not any(t == "INTERNAL_INFO" for t, _ in found(det, text))


# ---------------------------------------------------------------- 电话、指标、厂区、项目
def test_landline_with_split_digits(det):
    assert ("CONTACT", "021-6688-1234") in found(det, "请致电 021-6688-1234 联系")


@pytest.mark.parametrize("text, span", [
    ("本周一次合格率95.2%", "一次合格率95.2%"),
    ("报废率2.1%需关注", "报废率2.1%"),
    ("first-pass yield 93.5% this week", "first-pass yield 93.5%"),
    ("availability 88.0% on line 2", "availability 88.0%"),
])
def test_more_metric_names(det, text, span):
    assert ("METRIC", span) in found(det, text)


def test_plants_with_arabic_numerals_and_no_verb_prefix(det):
    assert ("PLANT", "临港3厂") in found(det, "货物在临港3厂待检")
    assert not any(t == "PLANT" for t, _ in found(det, "今天完成工厂巡检"))
    assert not any(t == "PLANT" for t, _ in found(det, "明天再做一次工厂盘点"))


def test_project_codes_with_trailing_numbers(det):
    assert ("PROJECT", "项目青松2") in found(det, "关于项目青松2的进度")
    assert ("PROJECT", "Project Falcon-7") in found(det, "Status of Project Falcon-7 is green")
