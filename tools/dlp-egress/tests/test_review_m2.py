"""R-20261118-M2：JSON键名必须检查，拒绝时不返回原值。"""
import json

import pytest

from agentnexus_dlp.models import EvaluateRequest
from bench.fake_secrets import AWS_DOC_EXAMPLE, GITHUB_A


def request(body):
    return EvaluateRequest("tenant-a", "actor-1", "review-m2", "external_api", "vendor-api",
                           "vendor_diagnostics", json_body=body)


@pytest.mark.parametrize("body,original", [
    ({AWS_DOC_EXAMPLE: "x"}, AWS_DOC_EXAMPLE),
    ({"items": [{GITHUB_A: 1}]}, GITHUB_A),
    ({"青川重工": 1}, "青川重工"),
    ({"to": {"nested": [{"项目白鹭": False}]}}, "项目白鹭"),
], ids=["secret-key", "nested-secret-key", "master-key", "routing-master-key"])
def test_dlp_m2_sensitive_json_keys_block_before_rendering(service, body, original):
    result = service.evaluate(request(body))
    assert result.decision == "BLOCK"
    assert result.ready_to_send is False
    assert original not in json.dumps(result.to_dict(), ensure_ascii=False)
    assert result.sanitized_json is None
    assert result.tokens == []
    assert service.revoke("tenant-a", "review-m2") == 0


def test_dlp_m2_clean_keys_and_nonstring_values_are_preserved(service):
    body = {"items": [{"sku": "CNC-07", "qty": 3, "active": True}], "note": None}
    result = service.evaluate(request(body))
    assert result.decision == "ALLOW"
    assert result.ready_to_send is True
    assert result.sanitized_json == body
