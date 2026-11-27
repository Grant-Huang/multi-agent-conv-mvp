#!/usr/bin/env python3
"""T1 Fast CI 路径分流判定（#432，设计 docs/fencing/INTEGRATION_PIPELINE.md 第 5 节）。

输入：改动文件清单（stdin，一行一个路径）与触发事件。
输出：各 CI 作业是否需要运行的 JSON（--json-out）+ 人类可读说明（stdout）。

规则（T1 分流，不改底线）：
- `push`（master）与未知事件一律全量：主干门禁不弱化；
- 共享或不确定路径（`.github/**`、`config/**`、`fixtures/**`、根下未知文件）一律全量；
- 只改 `docs/**`、`evals/**`、`**/*.md`（L0 路径）→ 代码作业全部跳过，
  `change-policy` 与 `security` 不受本脚本影响、每个 PR 照跑；
- 其余按前缀命中：Rust → `rust`；`web/**`、`docs/testing/**`（test:map 门）→ `web`；
  各 Python 工具目录 → 对应作业。

安全默认：本脚本自身崩溃或输入异常时**回退全量**——宁可多跑，不可漏跑。
"""
from __future__ import annotations

import argparse
import json
import sys
from typing import Iterable

# 任一命中即全量（影响所有作业的路径）。
SHARED_PREFIXES = (
    ".github/",
    "config/",
    "fixtures/",
    ".gitattributes",
)

# 作业 → 触发前缀。
JOB_PREFIXES: dict[str, tuple[str, ...]] = {
    "rust": ("crates/", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "web/src/bindings/"),
    "web": ("web/", "docs/testing/"),
    "python-websearch": ("tools/websearch/",),
    "python-dlp-egress": ("tools/dlp-egress/",),
    "python-tools": ("tools/ci/",),
}

# 不触发任何作业的 L0 路径（文档与评测数据）。
L0_PREFIXES = ("docs/", "evals/")


def _match(path: str, prefixes: tuple[str, ...]) -> bool:
    p = path
    # 只剥显式的 "./" 前缀，保留 ".github/" 这类以点开头的真实目录名。
    while p.startswith("./"):
        p = p[2:]
    return any(p == prefix or p.startswith(prefix) for prefix in prefixes)


def _all_true() -> dict[str, bool]:
    return {job: True for job in JOB_PREFIXES}


def triage(files: Iterable[str], event: str) -> tuple[dict[str, bool], list[str]]:
    """返回（作业→是否运行，逐条理由）。任何输入异常都视为全量。"""
    if event != "pull_request":
        return _all_true(), [f"事件={event}：非 PR，全量"]

    try:
        changed = [f.strip() for f in files if f and f.strip()]
    except Exception:  # noqa: BLE001 —— fail-safe 兜底
        return _all_true(), ["输入异常：全量"]

    result = {job: False for job in JOB_PREFIXES}
    reasons: list[str] = []
    if not changed:
        return result, ["改动清单为空：跳过全部代码作业"]

    full = False
    for path in changed:
        if _match(path, SHARED_PREFIXES):
            full = True
            reasons.append(f"{path} → 共享路径，全量")
            continue
        hit = False
        for job, prefixes in JOB_PREFIXES.items():
            if _match(path, prefixes):
                result[job] = True
                if not hit:
                    reasons.append(f"{path} → {job}")
                hit = True
        if hit:
            continue
        if _match(path, L0_PREFIXES) or path.endswith(".md"):
            reasons.append(f"{path} → L0 文档/评测，不触发代码作业")
            continue
        # 未登记的新路径不能被静默忽略。
        full = True
        reasons.append(f"{path} → 未登记路径，全量")

    if full:
        result = _all_true()
        reasons.append("结论：全量")
    else:
        skipped = [job for job, run in result.items() if not run]
        reasons.append(f"结论：仅运行 {[j for j, r in result.items() if r]}，跳过 {skipped}")
    return result, reasons


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", required=True, choices=["pull_request", "push", "other"])
    parser.add_argument("--json-out", help="把作业布尔写入该 JSON 文件")
    parser.add_argument("--quiet", action="store_true", help="不打印人类可读说明")
    args = parser.parse_args(argv)

    try:
        files = sys.stdin.read().splitlines()
        result, reasons = triage(files, args.event)
    except Exception:  # noqa: BLE001 —— 兜底：脚本任何异常都回退全量
        result, reasons = _all_true(), ["脚本异常：回退全量"]

    if args.json_out:
        try:
            with open(args.json_out, "w", encoding="utf-8") as fh:
                json.dump(result, fh, ensure_ascii=False, indent=2)
        except Exception:  # noqa: BLE001 —— JSON 落盘失败也不让作业红掉（stdout 仍有输出）
            print("警告：triage JSON 落盘失败", file=sys.stderr)

    if not args.quiet:
        for line in reasons:
            print(line)
        print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
