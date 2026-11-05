import pytest

from agent_websearch.errors import (
    BlockedUrl,
    FetchFailed,
    InvalidArgument,
    ProviderError,
    Timeout,
    WebSearchError,
)


@pytest.mark.parametrize(
    "cls, code",
    [
        (InvalidArgument, "invalid_argument"),
        (BlockedUrl, "blocked_url"),
        (Timeout, "timeout"),
        (ProviderError, "provider_error"),
        (FetchFailed, "fetch_failed"),
    ],
)
def test_error_codes_and_hierarchy(cls, code):
    err = cls("出错了")
    assert isinstance(err, WebSearchError)
    assert err.code == code
    assert err.message == "出错了"
    assert str(err) == "出错了"


def test_base_error_code_is_internal():
    assert WebSearchError("x").code == "internal"
