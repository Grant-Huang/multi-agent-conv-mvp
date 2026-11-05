"""运行配置：从环境变量读取，非法值回退默认值并记警告。"""
import logging
import os
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field, fields

from .models import METHOD_JINA, METHOD_LOCAL

logger = logging.getLogger("agent_websearch")

FETCH_ORDERS: dict[str, tuple[str, ...]] = {
    "local_first": (METHOD_LOCAL, METHOD_JINA),
    "jina_first": (METHOD_JINA, METHOD_LOCAL),
    "jina_only": (METHOD_JINA,),
    "local_only": (METHOD_LOCAL,),
}
EXTRACTORS = ("auto", "lxml", "trafilatura")
LOG_LEVELS = ("DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL")
_TRUE = {"1", "true", "yes", "on"}
_FALSE = {"0", "false", "no", "off"}


@dataclass(frozen=True)
class Settings:
    provider: str = "ddg"
    fetch_order: str = "local_first"
    extractor: str = "auto"
    search_timeout: float = 30
    fetch_timeout: float = 10
    jina_timeout: float = 20
    max_bytes: int = 3145728
    min_chars: int = 200
    allow_private: bool = False
    log_level: str = "WARNING"
    jina_api_key: str | None = field(default=None, repr=False)

    @property
    def fetch_methods(self) -> tuple[str, ...]:
        return FETCH_ORDERS[self.fetch_order]

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        env = os.environ if env is None else env
        defaults = {f.name: f.default for f in fields(cls)}
        values = {}
        for name, (key, parse) in _ENV_FIELDS.items():
            raw = env.get(key)
            if raw is None:
                continue
            try:
                values[name] = parse(raw)
            except ValueError:
                logger.warning("环境变量 %s 的值无效，使用默认值 %r", key, defaults[name])
        key = (env.get("JINA_API_KEY") or "").strip()
        values["jina_api_key"] = key or None
        return cls(**values)


def _choice(options: Mapping | tuple, upper: bool = False) -> Callable[[str], str]:
    def parse(raw: str) -> str:
        value = raw.strip().upper() if upper else raw.strip()
        if value not in options:
            raise ValueError
        return value
    return parse


def _positive_float(raw: str) -> float:
    value = float(raw)
    if value <= 0:
        raise ValueError
    return value


def _int_at_least(minimum: int) -> Callable[[str], int]:
    def parse(raw: str) -> int:
        value = int(raw.strip())
        if value < minimum:
            raise ValueError
        return value
    return parse


def _bool(raw: str) -> bool:
    value = raw.strip().lower()
    if value in _TRUE:
        return True
    if value in _FALSE:
        return False
    raise ValueError


def _non_empty(raw: str) -> str:
    value = raw.strip()
    if not value:
        raise ValueError
    return value


_ENV_FIELDS: dict[str, tuple[str, Callable[[str], object]]] = {
    "provider": ("WEBSEARCH_PROVIDER", _non_empty),
    "fetch_order": ("WEBSEARCH_FETCH_ORDER", _choice(FETCH_ORDERS)),
    "extractor": ("WEBSEARCH_EXTRACTOR", _choice(EXTRACTORS)),
    "search_timeout": ("WEBSEARCH_SEARCH_TIMEOUT", _positive_float),
    "fetch_timeout": ("WEBSEARCH_FETCH_TIMEOUT", _positive_float),
    "jina_timeout": ("WEBSEARCH_JINA_TIMEOUT", _positive_float),
    "max_bytes": ("WEBSEARCH_MAX_BYTES", _int_at_least(1)),
    "min_chars": ("WEBSEARCH_MIN_CHARS", _int_at_least(0)),
    "allow_private": ("WEBSEARCH_ALLOW_PRIVATE", _bool),
    "log_level": ("WEBSEARCH_LOG_LEVEL", _choice(LOG_LEVELS, upper=True)),
}


_HANDLER_FLAG = "_agent_websearch_stderr_handler"


def configure_logging(level: str) -> None:
    """日志只写 stderr（stdout 留给 CLI 的 JSON 和 MCP 协议）。重复调用会替换旧 handler。"""
    logger = logging.getLogger("agent_websearch")
    for handler in list(logger.handlers):
        if getattr(handler, _HANDLER_FLAG, False):
            logger.removeHandler(handler)
    handler = logging.StreamHandler(sys.stderr)
    setattr(handler, _HANDLER_FLAG, True)
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    logger.addHandler(handler)
    logger.setLevel(level)
    logger.propagate = False
