import io
import json
import subprocess
import sys
import types

import pytest

from agent_websearch import providers
from agent_websearch.errors import InvalidArgument, ProviderError, Timeout
from agent_websearch.models import SearchResult
from agent_websearch.providers import _ddg_worker, ddg


def completed(stdout="", returncode=0, stderr=""):
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr=stderr)


def raw(title, href, body="摘要"):
    return {"title": title, "href": href, "body": body}


# --- 注册表 ---

def test_ddg_is_registered_by_default():
    assert "ddg" in providers.provider_names()
    assert providers.get_provider("ddg").name == "ddg"


def test_unknown_provider_is_invalid_argument():
    with pytest.raises(InvalidArgument, match="nope"):
        providers.get_provider("nope")


def test_register_custom_provider(monkeypatch):
    monkeypatch.setattr(providers, "_REGISTRY", dict(providers._REGISTRY))

    class Fake:
        name = "fake"

        def search(self, query, max_results, timeout):
            return []

    providers.register(Fake())
    assert providers.get_provider("fake").name == "fake"
    assert "fake" in providers.provider_names()


# --- 父进程侧 ---

def test_search_parses_worker_output(monkeypatch):
    payload = [raw("标题一", "https://a.com/1", "摘要一"), raw("标题二", "https://b.com/2")]
    captured = {}

    def fake_run(cmd, **kwargs):
        captured.update(cmd=cmd, **kwargs)
        return completed(json.dumps(payload))

    monkeypatch.setattr(ddg.subprocess, "run", fake_run)
    results = ddg.DdgProvider().search("瀑布模型", 5, 12)
    assert results == [
        SearchResult("标题一", "https://a.com/1", "摘要一", "ddg", 1),
        SearchResult("标题二", "https://b.com/2", "摘要", "ddg", 2),
    ]
    assert captured["timeout"] == 12
    assert json.loads(captured["input"]) == {"query": "瀑布模型", "max_results": 5}


def test_query_is_passed_via_stdin_not_command_line(monkeypatch):
    captured = {}

    def fake_run(cmd, **kwargs):
        captured["cmd"] = cmd
        return completed("[]")

    monkeypatch.setattr(ddg.subprocess, "run", fake_run)
    ddg.DdgProvider().search("秘密查询词", 3, 10)
    assert "秘密查询词" not in " ".join(captured["cmd"])


def test_results_without_url_are_skipped_and_duplicates_removed_and_capped(monkeypatch):
    payload = [
        raw("无链接", ""),
        raw("非 http", "ftp://x.com/a"),
        raw("甲", "https://a.com/1"),
        raw("甲重复", "https://a.com/1"),
        raw("乙", "https://b.com/2"),
        raw("丙", "https://c.com/3"),
    ]
    monkeypatch.setattr(ddg.subprocess, "run", lambda *a, **k: completed(json.dumps(payload)))
    results = ddg.DdgProvider().search("q", 2, 10)
    assert [(r.rank, r.title) for r in results] == [(1, "甲"), (2, "乙")]


def test_missing_fields_become_empty_strings(monkeypatch):
    payload = [{"href": "https://a.com/1"}]
    monkeypatch.setattr(ddg.subprocess, "run", lambda *a, **k: completed(json.dumps(payload)))
    (result,) = ddg.DdgProvider().search("q", 3, 10)
    assert (result.title, result.snippet) == ("", "")


def test_timeout_raises_timeout(monkeypatch):
    def fake_run(cmd, **kwargs):
        raise subprocess.TimeoutExpired(cmd, kwargs["timeout"])

    monkeypatch.setattr(ddg.subprocess, "run", fake_run)
    with pytest.raises(Timeout, match="30"):
        ddg.DdgProvider().search("q", 3, 30)


def test_worker_error_message_is_reported(monkeypatch):
    out = json.dumps({"error": "RatelimitException: 被限流"})
    monkeypatch.setattr(ddg.subprocess, "run", lambda *a, **k: completed(out, returncode=1))
    with pytest.raises(ProviderError, match="被限流"):
        ddg.DdgProvider().search("q", 3, 10)


def test_worker_crash_without_json_reports_exit_code(monkeypatch):
    monkeypatch.setattr(
        ddg.subprocess, "run", lambda *a, **k: completed("", returncode=-11, stderr="segfault")
    )
    with pytest.raises(ProviderError, match="-11"):
        ddg.DdgProvider().search("q", 3, 10)


@pytest.mark.parametrize("stdout", ["not json", '{"a": 1}', '"text"'])
def test_unexpected_worker_output_is_provider_error(monkeypatch, stdout):
    monkeypatch.setattr(ddg.subprocess, "run", lambda *a, **k: completed(stdout))
    with pytest.raises(ProviderError):
        ddg.DdgProvider().search("q", 3, 10)


def test_real_subprocess_timeout_kills_a_hung_worker(monkeypatch):
    hung = [sys.executable, "-c", "import time; time.sleep(30)"]
    monkeypatch.setattr(ddg, "WORKER_COMMAND", hung)
    with pytest.raises(Timeout):
        ddg.DdgProvider().search("q", 3, 0.5)


def test_real_worker_module_rejects_bad_input_without_network(monkeypatch):
    proc = subprocess.run(
        ddg.WORKER_COMMAND, input="not json", capture_output=True, text=True, timeout=30
    )
    assert proc.returncode == 1
    assert "error" in json.loads(proc.stdout)


# --- 子进程侧（进程内调用 main，假的 ddgs） ---

def install_fake_ddgs(monkeypatch, text):
    module = types.ModuleType("ddgs")

    class DDGS:
        def text(self, query, **kwargs):
            return text(query, **kwargs)

    module.DDGS = DDGS
    monkeypatch.setitem(sys.modules, "ddgs", module)


def run_worker(stdin_text):
    out = io.StringIO()
    code = _ddg_worker.main(io.StringIO(stdin_text), out)
    return code, json.loads(out.getvalue())


def test_worker_returns_results(monkeypatch):
    seen = {}

    def text(query, **kwargs):
        seen.update(query=query, **kwargs)
        return [raw("t", "https://a.com")]

    install_fake_ddgs(monkeypatch, text)
    code, payload = run_worker(json.dumps({"query": "你好", "max_results": 4}))
    assert code == 0
    assert payload == [raw("t", "https://a.com")]
    assert seen == {"query": "你好", "max_results": 4}


def test_worker_treats_no_results_as_empty_list(monkeypatch):
    def text(query, **kwargs):
        raise Exception("No results found.")

    install_fake_ddgs(monkeypatch, text)
    assert run_worker(json.dumps({"query": "q", "max_results": 3})) == (0, [])


def test_worker_reports_search_exception(monkeypatch):
    def text(query, **kwargs):
        raise RuntimeError("连接被重置")

    install_fake_ddgs(monkeypatch, text)
    code, payload = run_worker(json.dumps({"query": "q", "max_results": 3}))
    assert code == 1
    assert payload["error"] == "RuntimeError: 连接被重置"


@pytest.mark.parametrize("stdin_text", [
    "", "not json", "[]", json.dumps({"query": 5, "max_results": 3}),
    json.dumps({"query": "q"}), json.dumps({"query": "q", "max_results": "3"}),
])
def test_worker_rejects_malformed_input(stdin_text):
    code, payload = run_worker(stdin_text)
    assert code == 1
    assert "error" in payload


def test_worker_reports_missing_ddgs(monkeypatch):
    monkeypatch.setitem(sys.modules, "ddgs", None)
    code, payload = run_worker(json.dumps({"query": "q", "max_results": 3}))
    assert code == 1
    assert "ddgs" in payload["error"]


# --- 重试：ddgs 的 auto 后端对同一查询时好时坏 ---

class RatelimitException(Exception):
    pass


class TimeoutException(Exception):
    pass


def scripted(outcomes):
    """依次返回 / 抛出 outcomes 中的结果，并记录调用次数。"""
    calls = {"n": 0}

    def text(query, **kwargs):
        outcome = outcomes[min(calls["n"], len(outcomes) - 1)]
        calls["n"] += 1
        if isinstance(outcome, Exception):
            raise outcome
        return outcome

    return text, calls


GOOD = [raw("t", "https://a.com")]


@pytest.mark.parametrize("first", [
    Exception("No results found."), [], RatelimitException("限流"), TimeoutException("超时"),
])
def test_worker_retries_transient_failures_until_success(monkeypatch, first):
    text, calls = scripted([first, GOOD])
    install_fake_ddgs(monkeypatch, text)
    assert run_worker(json.dumps({"query": "q", "max_results": 3})) == (0, GOOD)
    assert calls["n"] == 2


def test_worker_gives_up_after_three_attempts_with_empty_list(monkeypatch):
    text, calls = scripted([Exception("No results found.")])
    install_fake_ddgs(monkeypatch, text)
    assert run_worker(json.dumps({"query": "q", "max_results": 3})) == (0, [])
    assert calls["n"] == 3


def test_worker_reports_the_last_error_if_every_attempt_was_rate_limited(monkeypatch):
    text, calls = scripted([RatelimitException("限流")])
    install_fake_ddgs(monkeypatch, text)
    code, payload = run_worker(json.dumps({"query": "q", "max_results": 3}))
    assert code == 1
    assert payload["error"] == "RatelimitException: 限流"
    assert calls["n"] == 3


def test_worker_does_not_retry_other_errors(monkeypatch):
    text, calls = scripted([RuntimeError("连接被重置"), GOOD])
    install_fake_ddgs(monkeypatch, text)
    code, _ = run_worker(json.dumps({"query": "q", "max_results": 3}))
    assert code == 1
    assert calls["n"] == 1


def test_worker_sleeps_between_attempts_only(monkeypatch):
    sleeps = []
    monkeypatch.setattr(_ddg_worker.time, "sleep", sleeps.append)
    monkeypatch.setattr(_ddg_worker, "RETRY_DELAY", 0.7)
    text, _ = scripted([Exception("No results found.")])
    install_fake_ddgs(monkeypatch, text)
    run_worker(json.dumps({"query": "q", "max_results": 3}))
    assert sleeps == [0.7, 0.7]
