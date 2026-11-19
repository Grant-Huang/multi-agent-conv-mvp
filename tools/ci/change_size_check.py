"""变更规模检查：统计 PR 改动量，超过阈值时警告或阻断。

用法（在仓库根目录运行）：
    python3 tools/ci/change_size_check.py [--base origin/master] [--head HEAD] [--body-file pr.md]
PR 描述默认从环境变量 PR_BODY 读取（CI 中由 workflow 传入）。

规则（阈值依据：本仓库近 25 个非合并提交多为 500~1500 行，少数 3000 行以上）：
- 统计口径：新增 + 删除行数；排除生成文件和锁文件（见 common.GENERATED_PATTERNS）。
- 警告：≥ 800 行 或 ≥ 20 个文件。
- 阻断：≥ 2000 行 或 ≥ 40 个文件，除非 PR 描述包含 `## 大改动说明`（正文 ≥ 40 字）。
- 组件数 > 3（不计 docs / evals）：警告，建议拆分。
- 删除的测试多于新增的测试：阻断，除非 PR 描述包含 `## 测试删除说明`（正文 ≥ 20 字）。
  这是粗略检测（按测试标记行计数），目的是提醒人工确认，不是证明测试被弱化。
- 测试弱化信号（新增 `#[ignore]` / `.skip(` / `.only(` / `#[should_panic]`，或 assert
  净减少）：阻断，除非 PR 描述包含 `## 测试变更说明`（正文 ≥ 20 字）。
- 非纯文档改动缺少 `## 非目标` 或 Case/Feature ID：警告（不阻断）。

退出码：0 = 通过或仅警告；1 = 阻断；2 = 运行错误。
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from common import (  # noqa: E402
    FileChange,
    changed_files,
    component_of,
    diff_text,
    is_generated,
    pr_section,
    resolve_base,
)

WARN_LINES = 800
BLOCK_LINES = 2000
WARN_FILES = 20
BLOCK_FILES = 40
MAX_COMPONENTS = 3
# 不计入"组件数"的顶层目录：纯文档与评测数据不代表代码耦合。
COMPONENT_EXEMPT = {"docs", "evals"}

SIZE_NOTE_HEADING = "大改动说明"
SIZE_NOTE_MIN_CHARS = 40
TEST_NOTE_HEADING = "测试删除说明"
TEST_NOTE_MIN_CHARS = 20
WEAKEN_NOTE_HEADING = "测试变更说明"
WEAKEN_NOTE_MIN_CHARS = 20
SCOPE_NOTE_HEADING = "非目标"
CASE_ID_PATTERN = re.compile(
    r"\b(?:E2E|RT|HTTP|FT|AG|DES|INT)-[A-Z0-9-]+\b"
    r"|\bFeature\s*ID\s*[:：]|Case\s*ID\s*[:：]|功能\s*ID\s*[:：]",
    re.IGNORECASE,
)

# 测试标记：Rust #[test]、Python def test_、JS/TS it( / test( / describe(。
TEST_MARKER = re.compile(
    r"#\[(?:tokio::)?test\b|\bdef test_|^\s*(?:it|test|describe)\(|^\s*it\.skip\(|^\s*test\.skip\("
)
# 弱化 / 收窄测试的新增信号（只看 + 行）。
WEAKEN_ADDED = re.compile(
    r"#\[ignore\]|\.skip\(|\.only\(|#\[should_panic\b|xit\(|xdescribe\(|it\.only\(|test\.only\("
)
# assert 净减少：Rust assert! / assert_eq!、Python assert、Jest/Vitest expect(...).to*
ASSERT_LINE = re.compile(
    r"\bassert(?:_eq|_ne|_matches)?!|\bassert\s+|\.to(?:Be|Equal|StrictEqual|Contain|Match|Throw)\("
)
DOC_PREFIXES = ("docs/", "evals/")
DOC_SUFFIXES = (".md",)


@dataclass
class Finding:
    severity: str  # "warn" 或 "block"
    message: str


def is_doc_only(changes: list[FileChange]) -> bool:
    """改动是否全部落在文档 / 评测 / Markdown（不含生成文件）。"""
    counted = [c for c in changes if not is_generated(c.path)]
    if not counted:
        return True
    for c in counted:
        p = c.path
        if p.startswith(DOC_PREFIXES) or p.endswith(DOC_SUFFIXES):
            continue
        return False
    return True


def evaluate(
    changes: list[FileChange],
    pr_body: str | None,
    removed_tests: int = 0,
    added_tests: int = 0,
    weaken_added: int = 0,
    assert_added: int = 0,
    assert_removed: int = 0,
) -> dict:
    """纯函数：根据改动清单、PR 描述和测试标记计数给出结论。"""
    counted = [c for c in changes if not is_generated(c.path)]
    lines = sum(c.added + c.deleted for c in counted)
    files = len(counted)
    components = {component_of(c.path) for c in counted} - COMPONENT_EXEMPT

    size_note = pr_section(pr_body or "", SIZE_NOTE_HEADING)
    has_size_note = bool(size_note and len(size_note) >= SIZE_NOTE_MIN_CHARS)
    test_note = pr_section(pr_body or "", TEST_NOTE_HEADING)
    has_test_note = bool(test_note and len(test_note) >= TEST_NOTE_MIN_CHARS)
    weaken_note = pr_section(pr_body or "", WEAKEN_NOTE_HEADING)
    has_weaken_note = bool(weaken_note and len(weaken_note) >= WEAKEN_NOTE_MIN_CHARS)
    scope_note = pr_section(pr_body or "", SCOPE_NOTE_HEADING)
    has_scope_note = bool(scope_note and len(scope_note) >= 8)
    has_case_id = bool(CASE_ID_PATTERN.search(pr_body or ""))

    findings: list[Finding] = []

    # 规模：阻断与警告互斥，先判断阻断。
    if lines >= BLOCK_LINES or files >= BLOCK_FILES:
        msg = f"改动过大：{lines} 行、{files} 个文件（阻断线 {BLOCK_LINES} 行 / {BLOCK_FILES} 个文件）"
        if has_size_note:
            findings.append(Finding("warn", msg + "；已提供 `## 大改动说明`，请审查人确认"))
        else:
            findings.append(
                Finding("block", msg + "。请拆分 PR，或在描述中添加 `## 大改动说明`（≥40 字）")
            )
    elif lines >= WARN_LINES or files >= WARN_FILES:
        findings.append(
            Finding(
                "warn",
                f"改动较大：{lines} 行、{files} 个文件（警告线 {WARN_LINES} 行 / {WARN_FILES} 个文件）",
            )
        )

    if len(components) > MAX_COMPONENTS:
        findings.append(
            Finding(
                "warn",
                f"涉及 {len(components)} 个组件（{', '.join(sorted(components))}），"
                f"超过 {MAX_COMPONENTS} 个，建议按组件拆分",
            )
        )

    if removed_tests > added_tests:
        msg = f"删除的测试标记多于新增（删除 {removed_tests}，新增 {added_tests}）"
        if has_test_note:
            findings.append(Finding("warn", msg + "；已提供 `## 测试删除说明`，请审查人确认"))
        else:
            findings.append(
                Finding("block", msg + "。若是合理删除，请在描述中添加 `## 测试删除说明`（≥20 字）")
            )

    assert_net_loss = assert_removed > assert_added
    if weaken_added > 0 or assert_net_loss:
        parts = []
        if weaken_added > 0:
            parts.append(f"新增弱化标记 {weaken_added} 处")
        if assert_net_loss:
            parts.append(f"assert 净减少 {assert_removed - assert_added}")
        msg = "疑似测试弱化：" + "；".join(parts)
        if has_weaken_note:
            findings.append(Finding("warn", msg + "；已提供 `## 测试变更说明`，请审查人确认"))
        else:
            findings.append(
                Finding(
                    "block",
                    msg + "。请说明原因并添加 `## 测试变更说明`（≥20 字），或恢复断言强度",
                )
            )

    if counted and not is_doc_only(changes):
        if not has_scope_note:
            findings.append(
                Finding(
                    "warn",
                    "非文档改动缺少 `## 非目标`（≥8 字）。请写明本次明确不改什么，防止范围扩大",
                )
            )
        if not has_case_id:
            findings.append(
                Finding(
                    "warn",
                    "非文档改动未关联 Case/Feature ID（如 E2E-… / RT-… / Feature ID：…）",
                )
            )

    if any(f.severity == "block" for f in findings):
        status = "block"
    elif findings:
        status = "warn"
    else:
        status = "pass"

    return {
        "status": status,
        "lines": lines,
        "files": files,
        "components": sorted(components),
        "findings": [{"severity": f.severity, "message": f.message} for f in findings],
    }


def count_test_markers(diff: str) -> tuple[int, int]:
    """从 unified=0 的 diff 中统计新增与删除的测试标记行数。"""
    added = removed = 0
    in_hunk = False  # 只在 @@ 之后的行才是内容；文件头 "--- a/..." "+++ b/..." 必须跳过
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            in_hunk = False
            continue
        if line.startswith("@@"):
            in_hunk = True
            continue
        if not in_hunk:
            continue
        if line.startswith("+") and TEST_MARKER.search(line[1:]):
            added += 1
        elif line.startswith("-") and TEST_MARKER.search(line[1:]):
            removed += 1
    return added, removed


def count_weaken_signals(diff: str) -> tuple[int, int, int]:
    """返回 (弱化标记新增数, assert 新增数, assert 删除数)。"""
    weaken_added = assert_added = assert_removed = 0
    in_hunk = False
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            in_hunk = False
            continue
        if line.startswith("@@"):
            in_hunk = True
            continue
        if not in_hunk:
            continue
        if line.startswith("+"):
            body = line[1:]
            if WEAKEN_ADDED.search(body):
                weaken_added += 1
            if ASSERT_LINE.search(body):
                assert_added += 1
        elif line.startswith("-"):
            body = line[1:]
            if ASSERT_LINE.search(body):
                assert_removed += 1
    return weaken_added, assert_added, assert_removed


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="检查 PR 变更规模")
    parser.add_argument("--base", help="比较基线，默认 origin/$GITHUB_BASE_REF 或 origin/master")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--body-file", help="PR 描述文件；缺省时读取环境变量 PR_BODY")
    args = parser.parse_args(argv)

    if args.body_file:
        body = Path(args.body_file).read_text(encoding="utf-8")
    else:
        body = os.environ.get("PR_BODY", "")

    try:
        base = resolve_base(args.base)
        changes = changed_files(base, args.head)
        diff = diff_text(base, args.head) if changes else ""
    except RuntimeError as e:
        print(f"[change_size_check] 错误：{e}", file=sys.stderr)
        return 2

    added_tests, removed_tests = count_test_markers(diff)
    weaken_added, assert_added, assert_removed = count_weaken_signals(diff)
    result = evaluate(
        changes,
        body,
        removed_tests=removed_tests,
        added_tests=added_tests,
        weaken_added=weaken_added,
        assert_added=assert_added,
        assert_removed=assert_removed,
    )

    print(
        f"变更规模：{result['lines']} 行，{result['files']} 个文件，"
        f"{len(result['components'])} 个组件 → {result['status']}"
    )
    in_actions = os.environ.get("GITHUB_ACTIONS") == "true"
    for f in result["findings"]:
        print(f"  [{f['severity']}] {f['message']}")
        if in_actions:
            # GitHub 注解：在 PR 的 Files 页面和检查日志中高亮显示。
            level = "error" if f["severity"] == "block" else "warning"
            print(f"::{level}::{f['message']}")

    return 1 if result["status"] == "block" else 0


if __name__ == "__main__":
    sys.exit(main())
