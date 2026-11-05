import json
import logging
import subprocess
import sys

import pytest

from agent_websearch import cli, operations


@pytest.fixture
def recorded(monkeypatch):
    """替换 operations.run，记录 CLI 传下去的参数。"""
    calls = []
    reply = {"status": "success", "data": {"ok": True}, "message": ""}

    def fake_run(operation, **params):
        calls.append((operation, params))
        return reply

    monkeypatch.setattr(operations, "run", fake_run)
    return calls, reply


def run_cli(capsys, *argv):
    code = cli.main(list(argv))
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def test_search_fetch_defaults(recorded, capsys):
    calls, _ = recorded
    code, out, _ = run_cli(capsys, "search-fetch", "瀑布模型")
    assert code == 0
    assert calls == [("search_and_fetch", {
        "query": "瀑布模型", "max_results": 3, "max_chars_per_page": 3000, "provider": None,
    })]
    assert json.loads(out)["data"] == {"ok": True}


def test_search_fetch_options(recorded, capsys):
    calls, _ = recorded
    run_cli(capsys, "search-fetch", "q", "-n", "5", "--max-chars", "1200", "--provider", "exa")
    assert calls[0][1] == {
        "query": "q", "max_results": 5, "max_chars_per_page": 1200, "provider": "exa",
    }


def test_search_defaults_and_options(recorded, capsys):
    calls, _ = recorded
    run_cli(capsys, "search", "q")
    run_cli(capsys, "search", "q", "-n", "12", "--provider", "ddg")
    assert calls[0] == ("search", {"query": "q", "max_results": 8, "provider": None})
    assert calls[1] == ("search", {"query": "q", "max_results": 12, "provider": "ddg"})


def test_fetch_defaults_and_options(recorded, capsys):
    calls, _ = recorded
    run_cli(capsys, "fetch", "https://example.com")
    run_cli(capsys, "fetch", "https://example.com", "--max-chars", "2000")
    assert calls[0] == ("fetch", {"url": "https://example.com", "max_chars": 8000})
    assert calls[1] == ("fetch", {"url": "https://example.com", "max_chars": 2000})


def test_stdout_is_exactly_one_json_line_with_readable_chinese(recorded, capsys):
    calls, reply = recorded
    reply["data"] = {"title": "瀑布模型"}
    _, out, _ = run_cli(capsys, "fetch", "https://example.com")
    assert out.count("\n") == 1 and out.endswith("\n")
    assert "瀑布模型" in out


def test_error_envelope_gives_exit_code_1(recorded, capsys):
    _, reply = recorded
    reply.update(status="error", data={"code": "timeout"}, message="超时")
    code, out, _ = run_cli(capsys, "fetch", "https://example.com")
    assert code == 1
    assert json.loads(out)["data"]["code"] == "timeout"


@pytest.mark.parametrize("argv", [
    [], ["search"], ["fetch"], ["bogus", "x"], ["search", "q", "-n", "abc"],
    ["fetch", "u", "--max-chars", "1.5"],
])
def test_bad_arguments_exit_with_2(argv, recorded):
    with pytest.raises(SystemExit) as info:
        cli.main(argv)
    assert info.value.code == 2


def test_logging_goes_to_stderr_only(recorded, capsys, clean_env):
    clean_env.setenv("WEBSEARCH_LOG_LEVEL", "INFO")
    calls, reply = recorded

    def noisy_run(operation, **params):
        logging.getLogger("agent_websearch").info("提示信息")
        return reply

    clean_env.setattr(operations, "run", noisy_run)
    _, out, err = run_cli(capsys, "fetch", "https://example.com")
    assert "提示信息" not in out
    assert "提示信息" in err


def test_python_dash_m_runs_end_to_end_without_network():
    proc = subprocess.run(
        [sys.executable, "-m", "agent_websearch.cli", "fetch", "ftp://example.com/file"],
        capture_output=True, text=True, timeout=60,
    )
    assert proc.returncode == 1
    lines = proc.stdout.strip().splitlines()
    assert len(lines) == 1
    payload = json.loads(lines[0])
    assert payload["status"] == "error"
    assert payload["data"]["code"] == "blocked_url"
