"""搜索来源：统一接口 + 注册表。新增来源只需实现 `SearchProvider` 并 `register`。"""
from typing import Protocol

from ..errors import InvalidArgument
from ..models import SearchResult


class SearchProvider(Protocol):
    name: str

    def search(self, query: str, max_results: int, timeout: float) -> list[SearchResult]: ...


_REGISTRY: dict[str, SearchProvider] = {}


def register(provider: SearchProvider) -> None:
    _REGISTRY[provider.name] = provider


def get_provider(name: str) -> SearchProvider:
    try:
        return _REGISTRY[name]
    except KeyError:
        known = ", ".join(sorted(_REGISTRY)) or "无"
        raise InvalidArgument(f"未知的搜索来源：{name}（可用：{known}）") from None


def provider_names() -> list[str]:
    return sorted(_REGISTRY)


def _register_builtin() -> None:
    from .ddg import DdgProvider
    from .exa import ExaProvider
    from .parallel import ParallelProvider

    register(DdgProvider())
    register(ExaProvider())
    register(ParallelProvider())


_register_builtin()
