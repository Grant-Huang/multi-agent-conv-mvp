"""D1 接口定稿（2026-11-18）验收测试。

对应 docs/INTERFACE.md 第 9 节的决定：
9.1 统一返回外壳；9.3 channel 固定取值（含 agent）；9.4 request_id；9.6 只在 local_ui 恢复；
9.7 载荷上限 256 KB；9.8 api_version；9.9 结构化返回 + output schema。9.5（commit）D1 不做。
"""
import asyncio
import json
import os
import sys
from pathlib import Path

import pytest

from agentnexus_dlp.models import API_VERSION, CHANNELS, MAX_PAYLOAD_BYTES, EvaluateRequest
from agentnexus_dlp.policy import Policy

pytest.importorskip("mcp")

from mcp import ClientSession, StdioServerParameters  # noqa: E402
from mcp.client.stdio import stdio_client  # noqa: E402

from tests.conftest import TEST_VAULT_KEY  # noqa: E402

CONFIG_DIR = Path(__file__).resolve().parent.parent / "config"


def req(**kw):
    base = dict(tenant_id="t", actor_id="a", task_id="task-1", channel="external_api", destination_id="vendor-api",
                purpose="p", text="关联WO-559120")
    base.update(kw)
    return EvaluateRequest(**base)


# ---------------------------------------------------------------- 服务层
def test_constants():
    assert API_VERSION == "1.0"
    assert CHANNELS == ("email", "slack", "external_api", "file_upload", "mcp", "agent")
    assert MAX_PAYLOAD_BYTES == 256 * 1024


def test_request_id_always_present_and_vault_ref_removed(service):
    ok = service.evaluate(req()).to_dict()
    blocked = service.evaluate(req(destination_id="nowhere")).to_dict()
    for d in (ok, blocked):
        assert d["request_id"].startswith("req_")
        assert "vault_ref" not in d
    assert ok["request_id"] != blocked["request_id"]


def test_agent_channel_accepted_and_unknown_channel_blocked(service):
    assert service.evaluate(req(channel="agent")).decision == "ANONYMIZE"
    r = service.evaluate(req(channel="carrier_pigeon"))
    assert r.decision == "BLOCK"
    assert "invalid_channel" in r.reason_codes
    assert r.sanitized_text is None


@pytest.mark.parametrize("kind", ["text", "json"])
def test_payload_over_256kb_blocked(service, kind):
    big = "设备保养通用检查表。" * 30000  # 约 300 KB（UTF-8）
    kw = {"text": big} if kind == "text" else {"text": None, "json_body": {"note": big}}
    r = service.evaluate(req(**kw))
    assert r.decision == "BLOCK"
    assert "payload_too_large" in r.reason_codes
    assert r.sanitized_text is None and r.sanitized_json is None


def test_restore_only_in_local_ui(service):
    r = service.evaluate(req())
    assert Policy.load().allowed_restore_contexts == ["local_ui"]
    refused = service.restore("t", "a", "task-1", r.sanitized_text, "task_postprocess")
    assert refused.restored == 0 and refused.refused_reason == "context_not_allowed"
    assert service.restore("t", "a", "task-1", r.sanitized_text, "local_ui").text == "关联WO-559120"


# ---------------------------------------------------------------- MCP 层
def _params(tmp_path, allow_restore="0"):
    env = {k: v for k, v in os.environ.items() if not k.startswith("DLP_")}
    env.update({"DLP_TENANT_ID": "t", "DLP_ACTOR_ID": "a", "DLP_VAULT_PATH": str(tmp_path / "v.sqlite3"),
                "DLP_VAULT_KEY": TEST_VAULT_KEY.decode(), "DLP_ALLOW_RESTORE": allow_restore,
                "DLP_DESTINATIONS_FILE": str(CONFIG_DIR / "destinations.example.yaml")})
    return StdioServerParameters(command=sys.executable, args=["-m", "agentnexus_dlp.mcp_server"], env=env)


def _run(params, fn):
    async def inner():
        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as s:
                await s.initialize()
                return await fn(s)
    return asyncio.run(asyncio.wait_for(inner(), timeout=120))


def _validate(schema, value):
    jsonschema = pytest.importorskip("jsonschema")
    jsonschema.validate(value, schema)


def test_tools_publish_output_schema_and_structured_envelope(tmp_path):
    async def fn(s):
        tools = {t.name: t for t in (await s.list_tools()).tools}
        for name in ("egress_evaluate", "egress_restore", "egress_revoke"):
            assert tools[name].output_schema, name
        r = await s.call_tool("egress_evaluate", {"task_id": "t1", "channel": "agent",
                                                  "destination_id": "vendor-api", "purpose": "p",
                                                  "json_body": {"order": "WO-559120"}})
        sc = r.structured_content
        _validate(tools["egress_evaluate"].output_schema, sc)
        assert sc["api_version"] == "1.0" and sc["status"] == "success" and sc["error"] is None
        assert sc["data"]["decision"] == "ANONYMIZE"
        assert sc["data"]["sanitized_json"] == {"order": "[ORDER_ID_001]"}
        assert sc["data"]["request_id"].startswith("req_")
        # 文本内容与结构化内容一致，便于不支持结构化返回的客户端
        assert json.loads(r.content[0].text) == sc

        bad = await s.call_tool("egress_evaluate", {"task_id": "t1", "channel": "fax", "destination_id": "vendor-api",
                                                    "purpose": "p", "text": "x"})
        _validate(tools["egress_evaluate"].output_schema, bad.structured_content)
        assert bad.structured_content["data"]["decision"] == "BLOCK"

        rs = await s.call_tool("egress_restore", {"task_id": "t1", "text": "[ORDER_ID_001]"})
        _validate(tools["egress_restore"].output_schema, rs.structured_content)
        assert rs.structured_content["status"] == "error"
        assert rs.structured_content["error"]["code"] == "restore_disabled"
        assert rs.structured_content["data"] is None

        rv = await s.call_tool("egress_revoke", {"task_id": "t1"})
        _validate(tools["egress_revoke"].output_schema, rv.structured_content)
        assert rv.structured_content["data"]["revoked"] == 1
        return True
    assert _run(_params(tmp_path), fn)


def test_restore_enabled_uses_local_ui_by_default(tmp_path):
    async def fn(s):
        await s.call_tool("egress_evaluate", {"task_id": "t1", "channel": "mcp", "destination_id": "vendor-api",
                                              "purpose": "p", "text": "关联WO-559120"})
        rs = await s.call_tool("egress_restore", {"task_id": "t1", "text": "结果 [ORDER_ID_001]"})
        assert rs.structured_content["status"] == "success"
        assert rs.structured_content["data"]["text"] == "结果 WO-559120"
        other = await s.call_tool("egress_restore", {"task_id": "t1", "text": "[ORDER_ID_001]",
                                                     "context": "task_postprocess"})
        assert other.structured_content["data"]["refused_reason"] == "context_not_allowed"
        return True
    assert _run(_params(tmp_path, allow_restore="1"), fn)
