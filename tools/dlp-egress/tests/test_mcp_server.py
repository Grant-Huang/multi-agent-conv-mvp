"""MCP 协议级测试：真实启动 stdio 子进程，用 MCP 客户端完成握手、发现与调用。

这与 AgentNexus 接入 stdio MCP 服务的方式一致（initialize → tools/list → tools/call）。
"""
import asyncio
import json
import os
import sys
from pathlib import Path

import pytest
from bench.fake_secrets import STRIPE_LIVE  # noqa: E402

pytest.importorskip("mcp")

from mcp import ClientSession, StdioServerParameters  # noqa: E402
from mcp.client.stdio import stdio_client  # noqa: E402

from tests.conftest import TEST_VAULT_KEY  # noqa: E402

CONFIG_DIR = Path(__file__).resolve().parent.parent / "config"


def server_params(tmp_path, allow_restore="0"):
    env = dict(os.environ)
    env.update({
        "DLP_TENANT_ID": "tenant-a",
        "DLP_ACTOR_ID": "agentnexus-general",
        "DLP_VAULT_PATH": str(tmp_path / "vault.sqlite3"),
        "DLP_VAULT_KEY": TEST_VAULT_KEY.decode(),
        "DLP_ALLOW_AGENT_RESTORE": allow_restore,
        "DLP_DESTINATIONS_FILE": str(CONFIG_DIR / "destinations.example.yaml"),
    })
    return StdioServerParameters(command=sys.executable, args=["-m", "agentnexus_dlp.mcp_server"], env=env)


async def session_run(params, fn):
    # 整体超时，避免子进程挂住测试
    async def inner():
        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                return await fn(session)
    return await asyncio.wait_for(inner(), timeout=120)


def payload(result):
    """解开 D1 定稿的统一外壳：成功时返回 data，失败时返回整个外壳（含 status/error）。"""
    assert not result.is_error
    env = json.loads(result.content[0].text)
    assert env["api_version"] == "1.0"
    return env["data"] if env["status"] == "success" else env


def test_lists_tools_and_evaluates(tmp_path):
    async def fn(session):
        tools = {t.name: t for t in (await session.list_tools()).tools}
        assert {"egress_evaluate", "egress_restore", "egress_revoke"} <= set(tools)
        assert "tenant_id" not in tools["egress_evaluate"].input_schema["properties"]
        r = await session.call_tool("egress_evaluate", {
            "task_id": "t1", "channel": "external_api", "destination_id": "vendor-api",
            "purpose": "vendor_diagnostics",
            "text": f"Authorization: Bearer {STRIPE_LIVE} 发往供应商"})
        data = payload(r)
        assert data["decision"] == "BLOCK"
        assert STRIPE_LIVE not in r.content[0].text
        r2 = await session.call_tool("egress_evaluate", {
            "task_id": "t1", "channel": "external_api", "destination_id": "vendor-api",
            "purpose": "vendor_diagnostics", "json_body": {"order": "WO-559120", "yield": "良率97.5%"}})
        d2 = payload(r2)
        assert d2["decision"] == "ANONYMIZE"
        assert d2["sanitized_json"] == {"order": "[ORDER_ID_001]", "yield": "良率97.5%"}
        return True
    assert asyncio.run(session_run(server_params(tmp_path), fn))


def test_restore_disabled_for_agents_by_default(tmp_path):
    async def fn(session):
        await session.call_tool("egress_evaluate", {
            "task_id": "t1", "channel": "external_api", "destination_id": "vendor-api",
            "purpose": "x", "text": "关联WO-559120"})
        r = await session.call_tool("egress_restore", {"task_id": "t1", "text": "[ORDER_ID_001]"})
        data = payload(r)
        assert data["status"] == "error"
        assert "WO-559120" not in r.content[0].text
        return True
    assert asyncio.run(session_run(server_params(tmp_path), fn))


def test_restore_when_enabled(tmp_path):
    async def fn(session):
        await session.call_tool("egress_evaluate", {
            "task_id": "t1", "channel": "external_api", "destination_id": "vendor-api",
            "purpose": "x", "text": "关联WO-559120"})
        r = await session.call_tool("egress_restore", {"task_id": "t1", "text": "结果：[ORDER_ID_001]"})
        assert payload(r)["text"] == "结果：WO-559120"
        return True
    assert asyncio.run(session_run(server_params(tmp_path, allow_restore="1"), fn))


def test_config_file_resolves_relative_paths_and_rejects_inline_key(tmp_path):
    from agentnexus_dlp.mcp_server import env_from_args
    cfg = tmp_path / "dlp.yaml"
    cfg.write_text("tenant_id: t\nactor_id: a\nvault_path: v.sqlite3\nvault_key_file: key.txt\n"
                   "allow_agent_restore: false\n", encoding="utf-8")
    env = env_from_args(["--config", str(cfg)], {"DLP_ACTOR_ID": "from-env"})
    assert env["DLP_VAULT_PATH"] == str(tmp_path / "v.sqlite3")
    assert env["DLP_VAULT_KEY_FILE"] == str(tmp_path / "key.txt")
    assert env["DLP_ALLOW_AGENT_RESTORE"] == "0"
    assert env["DLP_ACTOR_ID"] == "from-env"  # 环境变量优先
    cfg.write_text("vault_key: abc\n", encoding="utf-8")
    with pytest.raises(RuntimeError):
        env_from_args(["--config", str(cfg)], {})


def test_server_starts_from_config_file_only(tmp_path):
    (tmp_path / "key.txt").write_text(TEST_VAULT_KEY.decode(), encoding="utf-8")
    cfg = tmp_path / "dlp.yaml"
    cfg.write_text(f"tenant_id: t\nactor_id: a\nvault_path: v.sqlite3\nvault_key_file: key.txt\n"
                   f"destinations_file: {CONFIG_DIR / 'destinations.example.yaml'}\n", encoding="utf-8")
    env = {k: v for k, v in os.environ.items() if not k.startswith("DLP_")}
    params = StdioServerParameters(command=sys.executable,
                                   args=["-m", "agentnexus_dlp.mcp_server", "--config", str(cfg)], env=env)

    async def fn(session):
        r = await session.call_tool("egress_evaluate", {
            "task_id": "t1", "channel": "external_api", "destination_id": "vendor-api", "purpose": "x",
            "text": "关联WO-559120"})
        return payload(r)["sanitized_text"]
    assert asyncio.run(session_run(params, fn)) == "关联[ORDER_ID_001]"


def test_missing_identity_fails_closed_at_startup(tmp_path):
    import subprocess
    env = {k: v for k, v in os.environ.items() if not k.startswith("DLP_")}
    proc = subprocess.run([sys.executable, "-m", "agentnexus_dlp.mcp_server"], env=env, capture_output=True,
                          text=True, timeout=60, stdin=subprocess.DEVNULL)
    assert proc.returncode == 2
    assert "DLP_TENANT_ID" in proc.stderr
