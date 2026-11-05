import httpx
import pytest

from agent_websearch.errors import BlockedUrl, FetchFailed, Timeout
from agent_websearch.fetch_local import fetch_local
from agent_websearch.settings import Settings

URL = "https://example.com/post"
SETTINGS = Settings(min_chars=20, extractor="lxml")
BODY = "这是一段足够长的正文内容，用来通过最小字数检查。" * 3


def html_page(text=BODY, title="示例标题"):
    return f"<html><head><title>{title}</title></head><body><p>{text}</p></body></html>"


def reply(content, content_type="text/html; charset=utf-8", status=200, headers=None):
    all_headers = {"Content-Type": content_type, **(headers or {})}
    if isinstance(content, str):
        content = content.encode("utf-8")
    return httpx.Response(status, content=content, headers=all_headers)


def test_html_page_is_converted_to_markdown(mock_http):
    mock_http.get(URL).mock(return_value=reply(html_page()))
    final_url, title, content = fetch_local(URL, SETTINGS)
    assert final_url == URL
    assert title == "示例标题"
    assert content == BODY


def test_request_sends_browser_user_agent(mock_http):
    route = mock_http.get(URL).mock(return_value=reply(html_page()))
    fetch_local(URL, SETTINGS)
    assert "Mozilla" in route.calls.last.request.headers["user-agent"]


@pytest.mark.parametrize(
    "content_type", ["text/plain", "text/markdown; charset=utf-8", "application/json"]
)
def test_text_like_content_is_returned_as_is(mock_http, content_type):
    mock_http.get(URL).mock(return_value=reply(BODY, content_type))
    _, title, content = fetch_local(URL, SETTINGS)
    assert content == BODY
    assert title == ""


def test_missing_content_type_is_treated_as_html(mock_http):
    mock_http.get(URL).mock(return_value=httpx.Response(200, content=html_page().encode()))
    assert fetch_local(URL, SETTINGS)[2] == BODY


@pytest.mark.parametrize(
    "content_type", ["application/pdf", "image/png", "application/octet-stream"]
)
def test_unsupported_content_type_fails(mock_http, content_type):
    mock_http.get(URL).mock(return_value=reply(b"%PDF-1.4", content_type))
    with pytest.raises(FetchFailed, match="内容类型"):
        fetch_local(URL, SETTINGS)


@pytest.mark.parametrize("status", [403, 404, 429, 500, 521])
def test_http_errors_fail_with_status(mock_http, status):
    mock_http.get(URL).mock(return_value=reply("err", status=status))
    with pytest.raises(FetchFailed, match=str(status)):
        fetch_local(URL, SETTINGS)


def test_too_short_content_fails(mock_http):
    mock_http.get(URL).mock(return_value=reply(html_page("太短")))
    with pytest.raises(FetchFailed, match="过短"):
        fetch_local(URL, SETTINGS)


def test_download_is_capped_by_max_bytes(mock_http):
    mock_http.get(URL).mock(return_value=reply("a" * 5000, "text/plain"))
    _, _, content = fetch_local(URL, Settings(min_chars=20, max_bytes=1000))
    assert len(content) == 1000


def test_follows_redirects_and_reports_final_url(mock_http):
    mock_http.get("https://example.com/old").mock(
        return_value=httpx.Response(301, headers={"Location": "/post"})
    )
    mock_http.get(URL).mock(return_value=reply(html_page()))
    final_url, _, content = fetch_local("https://example.com/old", SETTINGS)
    assert final_url == URL
    assert content == BODY


def test_redirect_to_internal_address_is_blocked(mock_http):
    mock_http.get(URL).mock(
        return_value=httpx.Response(302, headers={"Location": "http://127.0.0.1:8200/v1/models"})
    )
    with pytest.raises(BlockedUrl):
        fetch_local(URL, SETTINGS)


def test_redirect_to_non_http_scheme_is_blocked(mock_http):
    mock_http.get(URL).mock(
        return_value=httpx.Response(302, headers={"Location": "file:///etc/passwd"})
    )
    with pytest.raises(BlockedUrl):
        fetch_local(URL, SETTINGS)


def test_too_many_redirects_fail(mock_http):
    mock_http.get(url__regex=r"https://example\.com/.*").mock(
        side_effect=lambda request: httpx.Response(
            302, headers={"Location": request.url.path + "x"}
        )
    )
    with pytest.raises(FetchFailed, match="重定向"):
        fetch_local(URL, SETTINGS)


def test_redirect_without_location_fails(mock_http):
    mock_http.get(URL).mock(return_value=httpx.Response(302))
    with pytest.raises(FetchFailed):
        fetch_local(URL, SETTINGS)


def test_timeout_maps_to_timeout_error(mock_http):
    mock_http.get(URL).mock(side_effect=httpx.ReadTimeout("slow"))
    with pytest.raises(Timeout):
        fetch_local(URL, SETTINGS)


def test_connection_error_maps_to_fetch_failed(mock_http):
    mock_http.get(URL).mock(side_effect=httpx.ConnectError("refused"))
    with pytest.raises(FetchFailed, match="ConnectError"):
        fetch_local(URL, SETTINGS)


def test_internal_start_url_is_blocked_before_any_request(mock_http):
    route = mock_http.get("http://127.0.0.1/").mock(return_value=reply(html_page()))
    with pytest.raises(BlockedUrl):
        fetch_local("http://127.0.0.1/", SETTINGS)
    assert route.call_count == 0


def test_allow_private_permits_internal_address(mock_http):
    mock_http.get("http://127.0.0.1/").mock(return_value=reply(html_page()))
    settings = Settings(min_chars=20, extractor="lxml", allow_private=True)
    assert fetch_local("http://127.0.0.1/", settings)[2] == BODY


def test_gbk_page_declared_in_header(mock_http):
    page = html_page().encode("gbk")
    mock_http.get(URL).mock(return_value=reply(page, "text/html; charset=gbk"))
    assert fetch_local(URL, SETTINGS)[2] == BODY


def test_gbk_page_declared_only_in_meta(mock_http):
    page = (
        '<html><head><meta charset="gb2312"><title>标题</title></head>'
        f"<body><p>{BODY}</p></body></html>"
    ).encode("gbk")
    mock_http.get(URL).mock(return_value=reply(page, "text/html"))
    assert fetch_local(URL, SETTINGS)[2] == BODY


def test_undeclared_gbk_page_is_detected(mock_http):
    page = html_page().encode("gbk")
    mock_http.get(URL).mock(return_value=reply(page, "text/html"))
    assert fetch_local(URL, SETTINGS)[2] == BODY
