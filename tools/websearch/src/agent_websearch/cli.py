"""命令行入口。stdout 只输出一行 JSON，日志走 stderr；成功退出码 0，失败 1，参数错误 2。"""
import argparse
import json
import sys

from . import api, operations
from .settings import Settings, configure_logging


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="websearch", description="网页搜索与抓取（输出一行 JSON）"
    )
    sub = parser.add_subparsers(dest="command", required=True)

    combined = sub.add_parser("search-fetch", help="搜索并抓取前几条结果的正文（推荐）")
    combined.add_argument("query")
    combined.add_argument("-n", "--max-results", type=int, default=api.DEFAULT_COMBINED_RESULTS)
    combined.add_argument("--max-chars", type=int, default=api.DEFAULT_COMBINED_CHARS,
                          help="每页正文最多保留的字数")
    combined.add_argument("--provider")

    search = sub.add_parser("search", help="只搜索，返回标题、URL、摘要")
    search.add_argument("query")
    search.add_argument("-n", "--max-results", type=int, default=api.DEFAULT_SEARCH_RESULTS)
    search.add_argument("--provider")

    fetch = sub.add_parser("fetch", help="抓取单个网页的正文（Markdown）")
    fetch.add_argument("url")
    fetch.add_argument("--max-chars", type=int, default=api.DEFAULT_FETCH_CHARS)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    configure_logging(Settings.from_env().log_level)
    if args.command == "search-fetch":
        envelope = operations.run(
            "search_and_fetch", query=args.query, max_results=args.max_results,
            max_chars_per_page=args.max_chars, provider=args.provider,
        )
    elif args.command == "search":
        envelope = operations.run(
            "search", query=args.query, max_results=args.max_results, provider=args.provider,
        )
    else:
        envelope = operations.run("fetch", url=args.url, max_chars=args.max_chars)
    print(json.dumps(envelope, ensure_ascii=False))
    return 0 if envelope["status"] == "success" else 1


if __name__ == "__main__":
    sys.exit(main())
