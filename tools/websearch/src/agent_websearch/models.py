"""对外返回的数据结构。全部不可变，`to_dict()` 得到可直接 JSON 序列化的普通数据。"""
from dataclasses import asdict, dataclass
from typing import NamedTuple

METHOD_LOCAL = "local"
METHOD_JINA = "jina"


class Fetched(NamedTuple):
    """单个抓取实现（本机 / Jina）的原始产出，api 再加工成 Page。"""

    final_url: str
    title: str
    content: str


class _ToDict:
    def to_dict(self) -> dict:
        return asdict(self)


@dataclass(frozen=True)
class SearchResult(_ToDict):
    title: str
    url: str
    snippet: str
    provider: str
    rank: int


@dataclass(frozen=True)
class Page(_ToDict):
    url: str
    final_url: str
    title: str
    content: str
    method: str
    chars: int
    truncated: bool
    elapsed_ms: int


@dataclass(frozen=True)
class FetchFailure(_ToDict):
    url: str
    code: str
    message: str


@dataclass(frozen=True)
class SearchAndFetchResult(_ToDict):
    query: str
    provider: str
    results: list[SearchResult]
    pages: list[Page]
    failures: list[FetchFailure]
