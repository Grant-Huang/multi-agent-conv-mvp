"""文档检查：只看本次 PR 新增或改动的内容，默认只警告、不阻断。

用法（在仓库根目录运行）：
    python3 tools/ci/doc_lint.py [--base origin/master] [--head HEAD]
                                 [--json-out out.json] [--strict]
PR 描述默认从环境变量 PR_BODY 读取；没有该变量时（本地运行）跳过 PR 正文检查。

检查项（对应 docs/DOC_SYSTEM_PLAN.md 第 4.3 节）：
1. broken-link：新增行里的相对链接指向不存在的文件，或锚点在目标文档中找不到。
2. decision-name / decision-header / decision-section：docs/decisions/ 下新增的决策文件
   命名不是 YYYY-MM-DD-主题.md，或缺开头表格字段（日期、来源、状态、影响范围、相关设计），
   或缺必备小节（背景、问题、决策依据、定了什么、没选什么、出问题先看）。写法见
   docs/decisions/README.md。
3. decisions-append：往 docs/DECISIONS.md 追加了新的编号条目（`### N.`）。2026-11-21 起
   新决策改写到 docs/decisions/；若编号与已有条目重复，会在说明里指出。
4. design-unregistered：新增的设计文档（文件名含 DESIGN，或 docs/features/ 下的特性档案）
   没有在 docs/DESIGN.md 第 0.3 节登记。
5. pr-design-consistency：PR 正文缺 `## 设计一致性` 一节（只改测试的 PR 除外）。

局限：锚点规则是 GitHub 规则的近似实现；只检查新增行里的链接，存量坏链不在此列。

过渡期（约一周）只警告：退出码恒为 0；之后在 CI 中加 `--strict`，有发现即退出码 1。
退出码：0 = 通过或仅警告；1 = `--strict` 且有发现；2 = 运行错误。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path, PurePosixPath
from urllib.parse import unquote

sys.path.insert(0, str(Path(__file__).resolve().parent))

from common import diff_text, pr_section, resolve_base, run_git  # noqa: E402
from injection_scan import parse_added_lines  # noqa: E402


@dataclass(frozen=True)
class Finding:
    rule: str
    path: str
    line: int
    detail: str


# ---------- 锚点 ----------

def github_slug(heading: str) -> str:
    """近似 GitHub 的标题锚点：小写；去掉标点，保留字母数字（含中文）、空格、连字符、下划线；空格变连字符。"""
    text = heading.strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


FENCE = re.compile(r"^\s*(```|~~~)")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")


def heading_anchors(text: str) -> set[str]:
    """文档里所有标题的锚点；跳过代码块；重名标题依次加 -1、-2。"""
    anchors: set[str] = set()
    seen: dict[str, int] = {}
    in_fence = False
    for line in text.splitlines():
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = HEADING.match(line)
        if not m:
            continue
        slug = github_slug(m.group(2))
        n = seen.get(slug, 0)
        seen[slug] = n + 1
        anchors.add(slug if n == 0 else f"{slug}-{n}")
    return anchors


# ---------- 1. 链接 ----------

LINK = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
INLINE_CODE = re.compile(r"`[^`]*`")
SCHEME = re.compile(r"^(?:[a-zA-Z][a-zA-Z0-9+.\-]*:|//)")


def check_added_links(added, root: Path) -> list[Finding]:
    """检查 Markdown 新增行里的相对链接与锚点。added 为 (路径, 行号, 内容)。"""
    found: list[Finding] = []
    anchor_cache: dict[Path, set[str]] = {}
    for path, lineno, text in added:
        if not path.endswith(".md"):
            continue
        for m in LINK.finditer(INLINE_CODE.sub("", text)):
            url = m.group(1)
            if SCHEME.match(url):
                continue
            target_part, _, anchor = url.partition("#")
            source = root / path
            target = (source.parent / unquote(target_part)).resolve() if target_part else source
            if not target.exists():
                found.append(Finding("broken-link", path, lineno, f"链接目标不存在：{url}"))
                continue
            if anchor and target.suffix == ".md" and target.is_file():
                if target not in anchor_cache:
                    anchor_cache[target] = heading_anchors(target.read_text(encoding="utf-8"))
                if unquote(anchor) not in anchor_cache[target]:
                    found.append(Finding("broken-link", path, lineno, f"锚点不存在：{url}"))
    return found


# ---------- 2. 决策文件格式 ----------

DECISION_NAME = re.compile(r"^\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*\.md$")
HEADER_FIELDS = ["日期", "来源", "状态", "影响范围", "相关设计"]
REQUIRED_SECTIONS = ["背景", "问题", "决策依据", "定了什么", "没选什么", "出问题先看"]


def check_decision_file(path: str, text: str) -> list[Finding]:
    """检查 docs/decisions/ 下一个新增决策文件的命名、开头表格与必备小节。"""
    name = PurePosixPath(path).name
    if name == "README.md":
        return []
    found: list[Finding] = []
    if not DECISION_NAME.match(name):
        found.append(Finding("decision-name", path, 1,
                             f"文件名应为 YYYY-MM-DD-主题.md（小写英文和连字符），实际是 {name}"))
    rows = {m.group(1).strip() for m in re.finditer(r"^\|\s*([^|]+?)\s*\|", text, re.MULTILINE)}
    missing_fields = [f for f in HEADER_FIELDS if f not in rows]
    if missing_fields:
        found.append(Finding("decision-header", path, 1,
                             f"开头表格缺字段：{'、'.join(missing_fields)}"))
    sections = {m.group(1).strip() for m in re.finditer(r"^##\s+(.+?)\s*$", text, re.MULTILINE)}
    missing_sections = [s for s in REQUIRED_SECTIONS if s not in sections]
    if missing_sections:
        found.append(Finding("decision-section", path, 1,
                             f"缺必备小节：{'、'.join(missing_sections)}（模板见 docs/decisions/README.md）"))
    return found


# ---------- 3. DECISIONS.md 追加 ----------

NUMBERED = re.compile(r"^###\s+(\d+)\.")


def check_decisions_append(added, head_text: str) -> list[Finding]:
    """往 docs/DECISIONS.md 新增 `### N.` 条目时警告；编号重复时在说明里指出。"""
    found: list[Finding] = []
    for path, lineno, text in added:
        if path != "docs/DECISIONS.md":
            continue
        m = NUMBERED.match(text)
        if not m:
            continue
        num = m.group(1)
        detail = (f"新增了第 {num} 条；2026-11-21 起新决策请写到 docs/decisions/"
                  "（一条一个文件，见该目录 README），DECISIONS.md 只给旧条目标“已取代”")
        count = len(re.findall(rf"^###\s+{num}\.", head_text, re.MULTILINE))
        if count > 1:
            detail += f"；第 {num} 号在 DECISIONS.md 中重复出现 {count} 次"
        found.append(Finding("decisions-append", path, lineno, detail))
    return found


# ---------- 4. 设计文档登记 ----------

EXCLUDED_DOC_DIRS = ("docs/testing/", "docs/review/", "docs/fix-plans/", "docs/explore/")


def _is_design_doc(path: str) -> bool:
    if not path.startswith("docs/") or not path.endswith(".md"):
        return False
    if path.startswith(EXCLUDED_DOC_DIRS):
        return False
    if path.startswith("docs/features/"):
        return True
    return "DESIGN" in PurePosixPath(path).name.upper() and path != "docs/DESIGN.md"


def check_design_registered(new_files: list[str], index_text: str) -> list[Finding]:
    """新增的设计文档须在 docs/DESIGN.md（第 0.3 节索引）中被引用。"""
    found: list[Finding] = []
    for path in new_files:
        if not _is_design_doc(path):
            continue
        rel = path.removeprefix("docs/")
        rel_dir = str(PurePosixPath(rel).parent) + "/"
        if rel in index_text or (path.startswith("docs/features/") and rel_dir in index_text):
            continue
        found.append(Finding("design-unregistered", path, 1,
                             "新增的设计文档没有在 docs/DESIGN.md 第 0.3 节（产品特性设计索引）登记"))
    return found


# ---------- 5. PR 正文 ----------

TEST_PATH = re.compile(r"(^|/)(tests?|e2e)/|(^|/)test_[^/]*$|\.spec\.[jt]sx?$|_test\.rs$")


def check_pr_body(body: str | None, changed_paths: list[str]) -> list[Finding]:
    """PR 正文须有 `## 设计一致性`；只改测试的 PR 与未提供正文（本地运行）时跳过。"""
    if body is None or not changed_paths:
        return []
    if all(TEST_PATH.search(p) for p in changed_paths):
        return []
    if pr_section(body, "设计一致性") is not None:
        return []
    return [Finding("pr-design-consistency", "(PR 正文)", 0,
                    "缺 `## 设计一致性` 一节：写明属于哪个特性、是否触及共享契约、是否偏离产品宪章"
                    "（见 .github/pull_request_template.md）")]


# ---------- 入口 ----------

def _added_files(base: str, head: str) -> list[str]:
    out = run_git(["diff", "--name-status", "--no-renames", f"{base}...{head}"])
    return [line.split("\t", 1)[1] for line in out.splitlines()
            if line.startswith("A\t")]


def _changed_paths(base: str, head: str) -> list[str]:
    out = run_git(["diff", "--name-only", "--no-renames", f"{base}...{head}"])
    return [p for p in out.splitlines() if p]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.is_file() else ""


def lint(base: str, head: str, root: Path, body: str | None) -> list[Finding]:
    added = parse_added_lines(diff_text(base, head))
    new_files = _added_files(base, head)
    findings = check_added_links(added, root)
    for path in new_files:
        if path.startswith("docs/decisions/") and path.endswith(".md"):
            findings += check_decision_file(path, _read(root / path))
    findings += check_decisions_append(added, _read(root / "docs" / "DECISIONS.md"))
    findings += check_design_registered(new_files, _read(root / "docs" / "DESIGN.md"))
    findings += check_pr_body(body, _changed_paths(base, head))
    return findings


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--json-out")
    ap.add_argument("--strict", action="store_true", help="有发现即退出码 1（过渡期结束后启用）")
    args = ap.parse_args(argv)
    try:
        findings = lint(resolve_base(args.base), args.head, Path.cwd(), os.environ.get("PR_BODY"))
    except (RuntimeError, OSError) as e:
        print(f"错误：{e}", file=sys.stderr)
        return 2

    for f in findings:
        loc = f.path if f.line == 0 else f"{f.path}:{f.line}"
        print(f"[警告] {loc} {f.rule}：{f.detail}")
    mode = "有发现即阻断" if args.strict else "只警告，不阻断"
    print(f"文档检查：{len(findings)} 条发现（{mode}）。")
    if args.json_out:
        Path(args.json_out).write_text(
            json.dumps([asdict(f) for f in findings], ensure_ascii=False, indent=2),
            encoding="utf-8")
    return 1 if (args.strict and findings) else 0


if __name__ == "__main__":
    sys.exit(main())
