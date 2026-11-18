"""策略、转换与保真校验测试（对应方案第 4、6 节）。"""
import json

import pytest

from agentnexus_dlp.models import EvaluateRequest
from bench.fake_secrets import STRIPE_LIVE  # noqa: E402


def req(text=None, json_body=None, destination="vendor-api", channel="external_api",
        constraints=(), task_id="task-1", tenant_id="tenant-a", purpose="vendor_diagnostics"):
    return EvaluateRequest(
        tenant_id=tenant_id, actor_id="actor-1", task_id=task_id, channel=channel,
        destination_id=destination, purpose=purpose, text=text, json_body=json_body,
        constraints=list(constraints),
    )


def test_clean_text_is_allowed_unchanged(service):
    r = service.evaluate(req("设备保养通用检查表包括清洁、润滑、紧固和记录。"))
    assert r.decision == "ALLOW"
    assert r.sanitized_text == "设备保养通用检查表包括清洁、润滑、紧固和记录。"
    assert r.ready_to_send is True


def test_identifiers_are_tokenized_metric_and_device_preserved(service):
    r = service.evaluate(req("将西岸三厂的CNC-07维护记录上传，关联WO-559120，客户青川重工，良率97.5%"))
    assert r.decision == "ANONYMIZE"
    assert r.sanitized_text == (
        "将[PLANT_001]的CNC-07维护记录上传，关联[ORDER_ID_001]，客户[CUSTOMER_001]，良率97.5%")
    assert r.ready_to_send is True
    # 响应里不能带原值
    assert "青川重工" not in json.dumps(r.to_dict(), ensure_ascii=False)


def test_secret_blocks_and_value_never_in_output(service):
    r = service.evaluate(req(f"工具参数 Authorization: Bearer {STRIPE_LIVE} 发往供应商"))
    assert r.decision == "BLOCK"
    assert r.ready_to_send is False
    assert STRIPE_LIVE not in json.dumps(r.to_dict(), ensure_ascii=False)
    assert "[BLOCKED_SECRET]" in r.sanitized_text


def test_confidential_requires_review_not_send(service):
    r = service.evaluate(req("请把青川重工的报价发出去，含内部报价底线"))
    assert r.decision == "REVIEW"
    assert r.ready_to_send is False
    assert "[REDACTED_CONFIDENTIAL]" in r.sanitized_text


def test_unknown_destination_is_blocked(service):
    r = service.evaluate(req("设备保养通用检查表", destination="random-pastebin"))
    assert r.decision == "BLOCK"
    assert "destination_unknown" in r.reason_codes


def test_amount_tokenized_by_default_preserved_only_with_constraint(service):
    text = "向合作商发送青川重工报价，金额56万元"
    r1 = service.evaluate(req(text))
    assert "[AMOUNT_001]" in r1.sanitized_text
    r2 = service.evaluate(req(text, constraints=["preserve:AMOUNT"], task_id="task-2"))
    assert "56万元" in r2.sanitized_text


def test_preserve_constraint_cannot_preserve_secret(service):
    r = service.evaluate(req(f"Authorization: Bearer {STRIPE_LIVE}", constraints=["preserve:SECRET"]))
    assert r.decision == "BLOCK"
    assert STRIPE_LIVE not in r.sanitized_text


def test_routing_email_in_tool_call_is_kept(service):
    text = 'Agent调用 send_email(to="buyer@acme-demo.com", body="青川重工 良率97.5% 联系 ops@acme-demo.com")'
    r = service.evaluate(req(text, destination="vendor-mail", channel="email"))
    assert 'to="buyer@acme-demo.com"' in r.sanitized_text
    assert "ops@acme-demo.com" not in r.sanitized_text
    assert "[EMAIL_001]" in r.sanitized_text


def test_same_entity_same_token_within_task(service):
    r = service.evaluate(req("青川重工的WO-559120已交付，青川重工确认"))
    assert r.sanitized_text.count("[CUSTOMER_001]") == 2


def test_json_payload_keeps_keys_and_routing_fields(service):
    body = {"to": "buyer@acme-demo.com", "client": "青川重工", "yield": "良率97.5%",
            "items": [{"order": "WO-559120", "qty": 3}]}
    r = service.evaluate(req(json_body=body))
    out = r.sanitized_json
    assert set(out) == set(body)
    assert out["to"] == "buyer@acme-demo.com"
    assert out["client"] == "[CUSTOMER_001]"
    assert out["yield"] == "良率97.5%"
    assert out["items"][0] == {"order": "[ORDER_ID_001]", "qty": 3}
    assert r.validation["schema_ok"] is True


def test_detector_failure_fails_closed(service, monkeypatch):
    def boom(*a, **k):
        raise RuntimeError("detector crashed")
    monkeypatch.setattr(service.detector, "detect", boom)
    r = service.evaluate(req("青川重工"))
    assert r.decision == "BLOCK"
    assert r.ready_to_send is False
    assert "internal_error_fail_closed" in r.reason_codes
    assert r.sanitized_text is None


def test_request_without_payload_is_blocked(service):
    r = service.evaluate(req())
    assert r.decision == "BLOCK"


@pytest.mark.parametrize("text", ["请把ＷＯ-５５９１２０发出", "客户\u200b青川重工"])
def test_unicode_obfuscation_is_normalized(service, text):
    # 全角字符与零宽字符不能绕过识别
    r = service.evaluate(req(text))
    assert r.decision != "ALLOW"
