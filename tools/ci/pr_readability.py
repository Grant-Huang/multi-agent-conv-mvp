#!/usr/bin/env python3
"""PR 写给人看的自动检查（#452，决策 2026-11-26-pr-readability）。

四类检查，**只警告不阻断**（是否升级为阻断由用户决定）：
1. 标题格式：应为 `[第N线][L0~L3] …` 或 `[审查][L0~L3] …`；看起来像分支名时另加提醒；
2. 缺「给审查人」：正文没有 `## 给审查人` 一节，或该节超过 3 行；
3. 未登记缩写：标题与「给审查人」中出现 `docs/GLOSSARY.md` 未登记的全大写缩写；
4. 风险等级不一致：标题里的 [Lx] 与 risk_classify 的结果不符时提醒更正。

输入：PR 标题/正文（环境变量 PR_TITLE / PR_BODY 或参数）、GLOSSARY 路径、可选的分级结果。
输出：人类可读警告 + `--json-out`（warnings 数组）。任何异常都以退出码 0 结束（不阻断 CI）。
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys

# 普适技术缩写白名单：GLOSSARY 只登记仓库语义缩写，这些通用词不警告。
BUILTIN_ALLOW = {
    "PR", "CI", "API", "URL", "URI", "ID", "UI", "UX", "JSON", "YAML", "TOML",
    "SQL", "HTTP", "SSE", "CSV", "TS", "JS", "CSS", "DOM", "UUID", "TODO",
    "MCP", "A2A", "CLI", "CPU", "IO", "OS", "DB", "RFC", "WIP", "UTC", "DST",
    "LLM", "SSH", "NOTE", "OK", "UTF", "ASCII", "TCP", "UDP", "PID", "DNS",
    "AKIA", "AWS", "GPT", "DTO", "QPS", "SLA", "NSEC", "EOF", "CRC",
    # 仓库自引用与等级/阶段编号（GLOSSARY 两张混淆表已解释，逐 token 比对无意义）
    "GLOSSARY", "CASES", "DESIGN", "README",
    "L0", "L1", "L2", "L3", "L4", "L5",
    "P0", "P1", "P2", "P3", "T0", "T1", "T2", "T3",
}

TITLE_RE = re.compile(r"^\[(第[1-6]线|审查)\]\[(L[0-3])\]\s+\S")
RISK_TAG_RE = re.compile(r"\[(L[0-3])\]")
BRANCHY_RE = re.compile(r"^(line\d|cursor/|fix/|feat/|ci/|docs/|test/|chore/|[a-z]+-uxfix)", re.I)
CAPS_TOKEN_RE = re.compile(r"\b([A-Z][A-Z0-9]{1,11}(?:-[A-Z0-9]+)*)\b")
REVIEWER_HEADING = "## 给审查人"


def _load_glossary_prefixes(path: str | None) -> set[str]:
    """GLOSSARY 表格行首的缩写/编号前缀（如 `| ACP`、`| R-YYYYMMDD-` → 取首个大写段）。"""
    prefixes: set[str] = set()
    if not path or not os.path.exists(path):
        return prefixes
    try:
        with open(path, encoding="utf-8") as fh:
            for line in fh:
                m = re.match(r"^\s*\|\s*[`*]{0,2}([A-Z][A-Z0-9-]{1,20})", line)
                if m:
                    prefixes.add(m.group(1).split("YYYY")[0].rstrip("-"))
    except Exception:  # noqa: BLE001 —— 术语表读不到时跳过缩写检查，不阻断
        return prefixes
    return prefixes


def _reviewer_section(body: str) -> str | None:
    """取 `## 给审查人` 一节的内容；没有该节返回 None。"""
    if REVIEWER_HEADING not in body:
        return None
    after = body.split(REVIEWER_HEADING, 1)[1]
    stop = re.search(r"^## ", after, re.M)
    return after[: stop.start()] if stop else after


def _registered(token: str, glossary: set[str]) -> bool:
    token_u = token.upper()
    if token_u in BUILTIN_ALLOW or token_u in glossary:
        return True
    head = re.split(r"[\d]", token_u)[0].rstrip("-")
    return bool(head) and (head in glossary or token_u.startswith(tuple(f"{g}-" for g in glossary)))


def check(
    title: str,
    body: str,
    glossary_path: str | None = None,
    risk_level: str | None = None,
) -> list[str]:
    warnings: list[str] = []
    glossary = _load_glossary_prefixes(glossary_path)

    # 1) 标题格式
    if not TITLE_RE.match(title or ""):
        warnings.append(
            "标题格式：建议 `[第N线][L0~L3] 摘要` 或 `[审查][L0~L3] 摘要`（归属与风险一眼可见）"
        )
        if BRANCHY_RE.match(title or ""):
            warnings.append("标题看起来像分支名/提交摘要，请改写成给审查人看的一句话")
    if title and not BRANCHY_RE.match(title):
        m = RISK_TAG_RE.search(title)
        if risk_level and m and m.group(1) != risk_level:
            # 4) 风险等级不一致（放在格式段内，与 [Lx] 标签直接相关）
            warnings.append(
                f"风险等级不一致：标题写 {m.group(1)}，risk_classify 判定 {risk_level}——请更正标题"
            )

    # 2) 给审查人
    section = _reviewer_section(body or "")
    if section is None:
        warnings.append("正文缺「## 给审查人」一节：请写清做了什么/为什么/需要审查人做什么")
    else:
        lines = [
            ln for ln in section.splitlines()
            if ln.strip() and not ln.strip().startswith("<!--")
        ]
        if len(lines) > 3:
            warnings.append(f"「## 给审查人」超过 3 行（当前 {len(lines)} 行）：只留审查人必须知道的三件事")

    # 3) 未登记缩写（标题 + 给审查人一节）
    scope = (title or "") + "\n" + (section or "")
    unknown = sorted(
        {
            t
            for t in CAPS_TOKEN_RE.findall(scope)
            if not _registered(t, glossary)
        }
    )
    if unknown:
        warnings.append(
            "未登记缩写（见 docs/GLOSSARY.md 登记规则）：" + "、".join(unknown)
        )
    return warnings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--title", default=os.environ.get("PR_TITLE", ""))
    parser.add_argument("--body-file", help="正文文件；缺省读环境变量 PR_BODY")
    parser.add_argument("--glossary", default="docs/GLOSSARY.md")
    parser.add_argument(
        "--risk-from",
        help="risk_classify 的 risk.json，读 level 做标题一致性检查",
    )
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    try:
        body = ""
        if args.body_file and os.path.exists(args.body_file):
            body = open(args.body_file, encoding="utf-8").read()
        elif os.environ.get("PR_BODY"):
            body = os.environ["PR_BODY"]
        risk = None
        if args.risk_from and os.path.exists(args.risk_from):
            with open(args.risk_from, encoding="utf-8") as fh:
                risk = json.load(fh).get("level")
        warnings = check(args.title or "", body, args.glossary, risk)
    except Exception as exc:  # noqa: BLE001 —— 检查器自身异常不阻断 CI
        print(f"pr_readability 内部异常（忽略）：{exc}", file=sys.stderr)
        return 0

    if args.json_out:
        try:
            with open(args.json_out, "w", encoding="utf-8") as fh:
                json.dump({"warnings": warnings}, fh, ensure_ascii=False, indent=2)
        except Exception as exc:  # noqa: BLE001
            print(f"pr_readability 落盘失败（忽略）：{exc}", file=sys.stderr)
    for w in warnings:
        print(f"⚠️ {w}")
    if not warnings:
        print("PR 可读性检查通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
