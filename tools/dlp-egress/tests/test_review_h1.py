"""R-20261118-H1：真实 evaluate 管线的嵌套路由载荷与漏分析保护。"""
import json

import pytest

from agentnexus_dlp.models import EvaluateRequest
from bench.fake_secrets import AWS_DOC_EXAMPLE, GITHUB_A


def request(body):
    return EvaluateRequest("tenant-a", "actor-1", "review-h1", "email", "vendor-api",
                           "vendor_diagnostics", json_body=body)


@pytest.mark.parametrize("body,secret", [
    ({"to": {"address": "x@example.com", "note": "客户青川重工 key " + AWS_DOC_EXAMPLE}},
     AWS_DOC_EXAMPLE),
    ({"attendees": [{"email": "x@example.com", "name": "青川重工", "token": GITHUB_A}]},
     GITHUB_A),
    ({"cc": [[{"nested": {"note": AWS_DOC_EXAMPLE}}]]}, AWS_DOC_EXAMPLE),
])
def test_dlp_h1_nested_routing_secret_is_blocked(service, body, secret):
    result = service.evaluate(request(body))
    assert result.decision == "BLOCK"
    assert result.ready_to_send is False
    output = json.dumps(result.to_dict(), ensure_ascii=False)
    assert secret not in output
    assert "青川重工" not in output


def test_dlp_h1_nested_routing_customer_is_anonymized(service):
    result = service.evaluate(request({"attendees": [{"name": "青川重工", "count": 2}]}))
    assert result.decision == "ANONYMIZE"
    assert result.ready_to_send is True
    assert result.sanitized_json == {"attendees": [{"name": "[CUSTOMER_001]", "count": 2}]}


def test_dlp_h1_missing_string_analysis_fails_closed(service, monkeypatch):
    monkeypatch.setattr(service, "_collect_json", lambda *args: None)
    result = service.evaluate(request({"note": AWS_DOC_EXAMPLE}))
    assert result.decision == "BLOCK"
    assert result.ready_to_send is False
    assert AWS_DOC_EXAMPLE not in json.dumps(result.to_dict(), ensure_ascii=False)
