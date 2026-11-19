"""R-20261118-M5：字段寻址不能用展示字符串当索引。"""
import copy

import pytest

from agentnexus_dlp.models import EvaluateRequest


def request(body):
    return EvaluateRequest("tenant-a", "actor-1", "review-m5", "external_api", "vendor-api",
                           "vendor_diagnostics", json_body=body)


@pytest.mark.parametrize("body,expected", [
    ({"a.b": "青川重工", "a": {"b": "hello"}},
     {"a.b": "[CUSTOMER_001]", "a": {"b": "hello"}}),
    ({"items[0]": "青川重工", "items": ["hello"]},
     {"items[0]": "[CUSTOMER_001]", "items": ["hello"]}),
    ({"": {"note": "青川重工"}, "note": "hello"},
     {"": {"note": "[CUSTOMER_001]"}, "note": "hello"}),
    ({"to": {"a.b": "青川重工", "a": {"b": "hello"}}},
     {"to": {"a.b": "[CUSTOMER_001]", "a": {"b": "hello"}}}),
], ids=["dot", "brackets", "empty-key", "routing-object"])
def test_dlp_m5_distinct_json_leaves_never_overwrite_each_other(service, body, expected):
    original = copy.deepcopy(body)
    result = service.evaluate(request(body))
    assert result.decision == "ANONYMIZE"
    assert result.ready_to_send is True
    assert result.sanitized_json == expected
    assert result.validation["schema_ok"] is True
    assert body == original


def test_dlp_m5_duplicate_analysis_path_blocks_before_vault_write(service, monkeypatch):
    collect = service._collect_json

    def duplicated(node, path, fields, dest, req):
        collect(node, path, fields, dest, req)
        fields.append(fields[0])

    monkeypatch.setattr(service, "_collect_json", duplicated)
    result = service.evaluate(request("青川重工"))
    assert result.decision == "BLOCK"
    assert result.ready_to_send is False
    assert result.sanitized_json is None
    assert result.tokens == []
    assert service.revoke("tenant-a", "review-m5") == 0


def test_dlp_m5_ordinary_field_path_keeps_legacy_string_format(service):
    result = service.evaluate(request({"items": [{"order": "WO-559120"}]}))
    assert result.decision == "ANONYMIZE"
    assert result.entities[0].field_path == "items[0].order"
