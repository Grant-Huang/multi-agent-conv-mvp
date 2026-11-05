import asyncio
import json
import sys

import pytest

pytest.importorskip("mcp.server.mcpserver")

from agent_websearch import mcp_server, operations  # noqa: E402


def call(server, name, arguments):
    result = asyncio.run(server.call_tool(name, arguments))
    assert result.is_error is False
    return json.loads(result.content[0].text)


@pytest.fixture
def server():
    return mcp_server.build_server()


def test_exposes_the_three_tools(server):
    tools = {t.name: t for t in asyncio.run(server.list_tools())}
    assert set(tools) == {"web_search_and_fetch", "web_search", "web_fetch"}
    assert tools["web_search_and_fetch"].input_schema["required"] == ["query"]
    assert tools["web_search"].input_schema["required"] == ["query"]
    assert tools["web_fetch"].input_schema["required"] == ["url"]


def test_descriptions_guide_the_model_to_the_combined_tool(server):
    tools = {t.name: t.description for t in asyncio.run(server.list_tools())}
    assert "preferred" in tools["web_search_and_fetch"].lower()
    assert "web_search_and_fetch" in tools["web_search"]


def test_default_arguments_match_the_design(server):
    tools = {t.name: t.input_schema["properties"] for t in asyncio.run(server.list_tools())}
    assert tools["web_search_and_fetch"]["max_results"]["default"] == 3
    assert tools["web_search_and_fetch"]["max_chars_per_page"]["default"] == 3000
    assert tools["web_search"]["max_results"]["default"] == 8
    assert tools["web_fetch"]["max_chars"]["default"] == 8000


def test_search_and_fetch_tool_delegates_to_operations(server, monkeypatch):
    seen = {}

    def fake_run(operation, **params):
        seen.update(operation=operation, **params)
        return {"status": "success", "data": {"query": params["query"]}, "message": ""}

    monkeypatch.setattr(operations, "run", fake_run)
    payload = call(server, "web_search_and_fetch", {"query": "瀑布模型", "max_results": 2})
    assert payload["data"]["query"] == "瀑布模型"
    assert seen == {
        "operation": "search_and_fetch", "query": "瀑布模型",
        "max_results": 2, "max_chars_per_page": 3000,
    }


def test_search_and_fetch_tool_passes_page_size(server, monkeypatch):
    seen = {}
    monkeypatch.setattr(
        operations, "run",
        lambda operation, **params: seen.update(params) or {"status": "success", "data": {}},
    )
    call(server, "web_search_and_fetch", {"query": "q", "max_chars_per_page": 1500})
    assert seen["max_chars_per_page"] == 1500


def test_search_tool_delegates_to_operations(server, monkeypatch):
    seen = {}

    def fake_run(operation, **params):
        seen.update(operation=operation, **params)
        return {"status": "success", "data": {}, "message": ""}

    monkeypatch.setattr(operations, "run", fake_run)
    call(server, "web_search", {"query": "q"})
    assert seen == {"operation": "search", "query": "q", "max_results": 8}


def test_fetch_tool_delegates_to_operations(server, monkeypatch):
    seen = {}

    def fake_run(operation, **params):
        seen.update(operation=operation, **params)
        return {"status": "success", "data": {}, "message": ""}

    monkeypatch.setattr(operations, "run", fake_run)
    call(server, "web_fetch", {"url": "https://example.com"})
    assert seen == {"operation": "fetch", "url": "https://example.com", "max_chars": 8000}


def test_errors_are_returned_as_readable_json_not_protocol_errors(server):
    payload = call(server, "web_fetch", {"url": "ftp://example.com/file"})
    assert payload["status"] == "error"
    assert payload["data"]["code"] == "blocked_url"
    assert payload["message"]


def test_chinese_is_not_escaped_in_tool_output(server, monkeypatch):
    monkeypatch.setattr(
        operations, "run",
        lambda operation, **params: {"status": "success", "data": {"t": "瀑布模型"}, "message": ""},
    )
    result = asyncio.run(server.call_tool("web_fetch", {"url": "https://example.com"}))
    assert "瀑布模型" in result.content[0].text


def test_main_runs_stdio_transport(monkeypatch):
    started = {}

    class FakeServer:
        def run(self, transport):
            started["transport"] = transport

    monkeypatch.setattr(mcp_server, "build_server", lambda: FakeServer())
    assert mcp_server.main() == 0
    assert started == {"transport": "stdio"}


def test_main_without_mcp_installed_prints_install_hint(monkeypatch, capsys):
    monkeypatch.setitem(sys.modules, "mcp.server.mcpserver", None)
    assert mcp_server.main() == 1
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "agent-websearch[mcp]" in captured.err


def test_real_stdio_protocol_round_trip():
    """起真实子进程，用 JSON-RPC 走一遍：握手、列工具、调用（不联网）。"""
    import subprocess

    proc = subprocess.Popen(
        [sys.executable, "-m", "agent_websearch.mcp_server"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
    )

    def send(message):
        proc.stdin.write(json.dumps(message) + "\n")
        proc.stdin.flush()

    def receive():
        return json.loads(proc.stdout.readline())

    try:
        send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"},
        }})
        assert receive()["result"]["serverInfo"]["name"] == "agent-websearch"
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        names = {t["name"] for t in receive()["result"]["tools"]}
        assert names == {"web_search_and_fetch", "web_search", "web_fetch"}
        send({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "web_fetch", "arguments": {"url": "ftp://example.com/a"},
        }})
        result = receive()["result"]
        assert json.loads(result["content"][0]["text"])["data"]["code"] == "blocked_url"
    finally:
        proc.stdin.close()
        proc.wait(timeout=15)
    assert proc.returncode == 0
