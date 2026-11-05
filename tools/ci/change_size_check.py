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

# 测试标记：Rust #[test]、Python def test_、JS/TS it( / test( / describe(。
TEST_MARKER = re.compile(
    r"#\[(?:tokio::)?test\b|\bdef test_|^\s*(?:it|test|describe)\(|^\s*it\.skip\(|^\s*test\.skip\("
)


@dataclass
class Finding:
    severity: str  # "warn" 或 "block"
    message: str


def evaluate(
    changes: list[FileChange],
    pr_body: str | None,
    removed_tests: int = 0,
    added_tests: int = 0,
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
    result = evaluate(changes, body, removed_tests=removed_tests, added_tests=added_tests)

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
