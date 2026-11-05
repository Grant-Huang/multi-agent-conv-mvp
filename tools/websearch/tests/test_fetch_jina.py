import logging

import httpx
import pytest

from agent_websearch.errors import FetchFailed, Timeout
from agent_websearch.fetch_jina import fetch_jina
from agent_websearch.settings import Settings

TARGET = "https://example.com/post"
JINA = "https://r.jina.ai/" + TARGET
SETTINGS = Settings(min_chars=20)
BODY = "这是 Jina 返回的 Markdown 正文内容，长度足够通过检查。" * 3
JINA_REPLY = (
    "Title: 示例标题\n\n"
    f"URL Source: {TARGET}/final\n\n"
    f"Markdown Content:\n{BODY}"
)


def test_success_parses_title_url_and_content(mock_http):
    mock_http.get(JINA).mock(return_value=httpx.Response(200, text=JINA_REPLY))
    final_url, title, content = fetch_jina(TARGET, SETTINGS)
    assert (final_url, title, content) == (TARGET + "/final", "示例标题", BODY)


def test_reply_without_header_is_used_whole(mock_http):
    mock_http.get(JINA).mock(return_value=httpx.Response(200, text=BODY))
    assert fetch_jina(TARGET, SETTINGS) == (TARGET, "", BODY)


def test_no_authorization_header_without_key(mock_http):
    route = mock_http.get(JINA).mock(return_value=httpx.Response(200, text=JINA_REPLY))
    fetch_jina(TARGET, SETTINGS)
    assert "authorization" not in route.calls.last.request.headers


def test_key_is_sent_as_bearer_token(mock_http):
    route = mock_http.get(JINA).mock(return_value=httpx.Response(200, text=JINA_REPLY))
    fetch_jina(TARGET, Settings(min_chars=20, jina_api_key="jina_secret"))
    assert route.calls.last.request.headers["authorization"] == "Bearer jina_secret"


@pytest.mark.parametrize("status", [401, 429, 500])
def test_http_errors_fail_without_leaking_key(mock_http, status, caplog):
    mock_http.get(JINA).mock(return_value=httpx.Response(status, text="denied jina_secret"))
    settings = Settings(min_chars=20, jina_api_key="jina_secret")
    with caplog.at_level(logging.DEBUG, logger="agent_websearch"):
        with pytest.raises(FetchFailed) as info:
            fetch_jina(TARGET, settings)
    assert str(status) in info.value.message
    assert "jina_secret" not in info.value.message
    assert "jina_secret" not in caplog.text


def test_upstream_error_warning_in_body_fails(mock_http):
    text = (
        "Title: x\n\nWarning: Target URL returned error 403: Forbidden\n\n"
        "Markdown Content:\n拒绝访问"
    ) * 3
    mock_http.get(JINA).mock(return_value=httpx.Response(200, text=text))
    with pytest.raises(FetchFailed, match="403"):
        fetch_jina(TARGET, SETTINGS)


def test_too_short_content_fails(mock_http):
    mock_http.get(JINA).mock(return_value=httpx.Response(200, text="Markdown Content:\n太短"))
    with pytest.raises(FetchFailed, match="过短"):
        fetch_jina(TARGET, SETTINGS)


def test_content_is_capped_by_max_bytes(mock_http):
    mock_http.get(JINA).mock(return_value=httpx.Response(200, text="a" * 5000))
    _, _, content = fetch_jina(TARGET, Settings(min_chars=20, max_bytes=1000))
    assert len(content) == 1000


def test_timeout_maps_to_timeout_error(mock_http):
    mock_http.get(JINA).mock(side_effect=httpx.ReadTimeout("slow"))
    with pytest.raises(Timeout):
        fetch_jina(TARGET, SETTINGS)


def test_connection_error_maps_to_fetch_failed(mock_http):
    mock_http.get(JINA).mock(side_effect=httpx.ConnectError("refused"))
    with pytest.raises(FetchFailed, match="ConnectError"):
        fetch_jina(TARGET, SETTINGS)
