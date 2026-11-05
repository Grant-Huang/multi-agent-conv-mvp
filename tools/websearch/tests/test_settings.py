import logging

import pytest

from agent_websearch.settings import FETCH_ORDERS, Settings


def test_defaults(clean_env):
    s = Settings.from_env()
    assert s.provider == "exa"
    assert s.fetch_order == "local_first"
    assert s.extractor == "auto"
    assert s.search_timeout == 30
    assert s.fetch_timeout == 10
    assert s.jina_timeout == 20
    assert s.max_bytes == 3145728
    assert s.min_chars == 200
    assert s.allow_private is False
    assert s.log_level == "WARNING"
    assert s.jina_api_key is None
    assert Settings() == s


def test_reads_environment(clean_env):
    clean_env.setenv("WEBSEARCH_PROVIDER", "ddg")
    clean_env.setenv("WEBSEARCH_FETCH_ORDER", "jina_first")
    clean_env.setenv("WEBSEARCH_EXTRACTOR", "lxml")
    clean_env.setenv("WEBSEARCH_SEARCH_TIMEOUT", "5.5")
    clean_env.setenv("WEBSEARCH_FETCH_TIMEOUT", "3")
    clean_env.setenv("WEBSEARCH_JINA_TIMEOUT", "7")
    clean_env.setenv("WEBSEARCH_MAX_BYTES", "1000")
    clean_env.setenv("WEBSEARCH_MIN_CHARS", "0")
    clean_env.setenv("WEBSEARCH_ALLOW_PRIVATE", "1")
    clean_env.setenv("WEBSEARCH_LOG_LEVEL", "debug")
    clean_env.setenv("JINA_API_KEY", "jina_secret")
    s = Settings.from_env()
    assert s.provider == "ddg"
    assert s.fetch_order == "jina_first"
    assert s.extractor == "lxml"
    assert s.search_timeout == 5.5
    assert s.fetch_timeout == 3
    assert s.jina_timeout == 7
    assert s.max_bytes == 1000
    assert s.min_chars == 0
    assert s.allow_private is True
    assert s.log_level == "DEBUG"
    assert s.jina_api_key == "jina_secret"


def test_accepts_explicit_mapping_instead_of_os_environ(clean_env):
    clean_env.setenv("WEBSEARCH_FETCH_ORDER", "jina_only")
    s = Settings.from_env({"WEBSEARCH_FETCH_ORDER": "local_only"})
    assert s.fetch_order == "local_only"


@pytest.mark.parametrize(
    "key, value, field, default",
    [
        ("WEBSEARCH_FETCH_ORDER", "random", "fetch_order", "local_first"),
        ("WEBSEARCH_EXTRACTOR", "bs4", "extractor", "auto"),
        ("WEBSEARCH_SEARCH_TIMEOUT", "abc", "search_timeout", 30),
        ("WEBSEARCH_SEARCH_TIMEOUT", "0", "search_timeout", 30),
        ("WEBSEARCH_FETCH_TIMEOUT", "-1", "fetch_timeout", 10),
        ("WEBSEARCH_MAX_BYTES", "1.5", "max_bytes", 3145728),
        ("WEBSEARCH_MAX_BYTES", "0", "max_bytes", 3145728),
        ("WEBSEARCH_MIN_CHARS", "-5", "min_chars", 200),
        ("WEBSEARCH_ALLOW_PRIVATE", "maybe", "allow_private", False),
        ("WEBSEARCH_LOG_LEVEL", "LOUD", "log_level", "WARNING"),
        ("WEBSEARCH_PROVIDER", "  ", "provider", "exa"),
    ],
)
def test_invalid_values_fall_back_to_default_with_warning(key, value, field, default, caplog):
    with caplog.at_level(logging.WARNING, logger="agent_websearch"):
        s = Settings.from_env({key: value})
    assert getattr(s, field) == default
    assert key in caplog.text


@pytest.mark.parametrize("raw, expected", [
    ("1", True), ("true", True), ("YES", True), ("on", True),
    ("0", False), ("false", False), ("No", False), ("off", False),
])
def test_bool_parsing(raw, expected):
    assert Settings.from_env({"WEBSEARCH_ALLOW_PRIVATE": raw}).allow_private is expected


def test_empty_jina_key_is_none():
    assert Settings.from_env({"JINA_API_KEY": "  "}).jina_api_key is None


def test_jina_key_never_appears_in_repr_or_logs(caplog):
    with caplog.at_level(logging.DEBUG, logger="agent_websearch"):
        s = Settings.from_env({"JINA_API_KEY": "jina_secret", "WEBSEARCH_MAX_BYTES": "bad"})
    assert "jina_secret" not in repr(s)
    assert "jina_secret" not in caplog.text


@pytest.mark.parametrize("order, methods", [
    ("local_first", ("local", "jina")),
    ("jina_first", ("jina", "local")),
    ("jina_only", ("jina",)),
    ("local_only", ("local",)),
])
def test_fetch_methods_follow_order(order, methods):
    assert FETCH_ORDERS[order] == methods
    assert Settings(fetch_order=order).fetch_methods == methods


# --- 日志配置 ---

@pytest.fixture
def pristine_logger():
    logger = logging.getLogger("agent_websearch")
    saved = (list(logger.handlers), logger.level, logger.propagate)
    yield logger
    logger.handlers[:] = saved[0]
    logger.setLevel(saved[1])
    logger.propagate = saved[2]


def test_configure_logging_writes_to_stderr_only(pristine_logger, capsys):
    from agent_websearch.settings import configure_logging

    configure_logging("INFO")
    pristine_logger.info("来自日志")
    captured = capsys.readouterr()
    assert "来自日志" in captured.err
    assert captured.out == ""
    assert pristine_logger.level == logging.INFO


def test_configure_logging_is_idempotent_and_follows_current_stderr(pristine_logger, capsys):
    from agent_websearch.settings import configure_logging

    configure_logging("WARNING")
    configure_logging("DEBUG")
    assert len(pristine_logger.handlers) == 1
    assert pristine_logger.level == logging.DEBUG
    pristine_logger.debug("只出现一次")
    assert capsys.readouterr().err.count("只出现一次") == 1


def test_configure_logging_does_not_leak_to_root_logger(pristine_logger):
    from agent_websearch.settings import configure_logging

    configure_logging("INFO")
    assert pristine_logger.propagate is False
