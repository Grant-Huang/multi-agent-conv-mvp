"""提示注入与工作流注入扫描：只检查 PR 的"新增行"。

用法（在仓库根目录运行）：
    python3 tools/ci/injection_scan.py [--base origin/master] [--head HEAD] [--json-out out.json]
PR 描述默认从环境变量 PR_BODY 读取。

检查项：
1. hidden-unicode（阻断）：新增行含零宽字符、双向控制符、Unicode 标签字符。
   这些字符肉眼看不见，却会被模型读到，是隐藏指令的常见载体。
2. workflow-untrusted-expr（阻断）：工作流中把 PR 标题/正文/分支名/评论等外部可控内容
   直接写进 `run:` 之类的位置。只允许赋值给环境变量（`NAME: ${{ ... }}`）。
3. injection-phrase：新增行含"忽略之前的指令""泄露密钥""curl | sh"等短语。
   - 位于代码、配置、评测数据、AGENTS.md / CLAUDE.md：阻断；
     PR 描述含 `## 注入扫描说明`（正文 ≥ 20 字）时降为警告。
   - 位于普通文档（docs/**、*.md）：只警告（文档常会讨论注入本身）。
4. html-comment（警告）：Markdown 新增了较长的 HTML 注释，渲染后看不见，但 Agent 读得到。

局限（务必知道）：这是基于规则的粗筛，只能拦住常见写法，拦不住改写、编码或多语言变体，
也看不到运行时进入系统的内容（网页、MCP 返回）。运行时的防护见 7-fence-review.md 第 4 节。

退出码：0 = 通过或仅警告；1 = 阻断；2 = 运行错误。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass, asdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from common import diff_text, path_matches, pr_section, resolve_base  # noqa: E402

# 零宽字符、双向控制符、Unicode 标签字符（U+E0000~E007F）。
HIDDEN_CHARS = re.compile(
    "[\u200b-\u200f\u202a-\u202e\u2060-\u2064\u2066-\u2069\ufeff\U000e0000-\U000e007f]"
)

# 外部可控的 GitHub 上下文：不能出现在 run 脚本里。
UNTRUSTED_EXPR = re.compile(
    r"\$\{\{\s*(?:github\.head_ref"
    r"|github\.event\.(?:pull_request\.(?:title|body|head\.(?:ref|label))"
    r"|issue\.(?:title|body)|comment\.body|review\.body|review_comment\.body"
    r"|head_commit\.(?:message|author\.(?:name|email))|commits\b[^}]*)"
    r")[^}]*\}\}"
)
# 形如 `NAME: ${{ ... }}` 的环境变量赋值是安全写法；但 `run:`、`script:` 本身会执行内容，不算。
ENV_ASSIGNMENT = re.compile(r"^\s*(?!(?:-\s*)?(?:run|script)\s*:)[A-Za-z_][A-Za-z0-9_]*:\s")

INJECTION_PHRASES = [
    ("忽略先前指令", re.compile(
        r"(?i)\b(ignore|disregard|forget)\b.{0,30}\b(previous|prior|above|earlier|all)\b"
        r".{0,20}\b(instruction|prompt|rule|direction)s?\b")),
    ("忽略先前指令（中文）", re.compile(
        r"(忽略|无视|忘记|不要遵守).{0,8}(以上|之前|前面|上述|先前|所有).{0,8}(指令|提示|规则|要求|约束)")),
    ("重设角色", re.compile(r"(?i)\byou\s+are\s+now\b|\bnew\s+instructions?\s*:|从现在起你是")),
    ("要求隐瞒用户", re.compile(
        r"(?i)\b(do\s+not|don't|never)\s+(tell|inform|mention|reveal\s+to)\s+the\s+user\b"
        r"|不要(告诉|通知|让)用户")),
    ("索要系统提示或凭证", re.compile(
        r"(?i)\b(reveal|print|show|output|leak|exfiltrate|send)\b.{0,40}"
        r"\b(system\s+prompt|api[_ -]?key|secret|token|password|credential)s?\b"
        r"|(泄露|输出|打印|发送|外传).{0,12}(系统提示|密钥|令牌|口令|凭证)")),
    ("下载并执行", re.compile(r"(?i)\b(curl|wget)\b[^|\n]*\|\s*(sudo\s+)?(ba|z)?sh\b")),
]

HTML_COMMENT = re.compile(r"<!--(.{20,}?)-->")

# 扫描时不看自己：脚本、测试夹具里必然包含这些短语。
PHRASE_EXEMPT = ["tools/ci/injection_scan.py", "tools/ci/tests/**"]
# 普通文档：短语命中只警告。AGENTS.md / CLAUDE.md 是 Agent 指令入口，不算普通文档。
DOC_PATHS = ["docs/**", "**/*.md"]
INSTRUCTION_FILES = ["AGENTS.md", "CLAUDE.md", "**/AGENTS.md", "**/CLAUDE.md"]

OVERRIDE_HEADING = "注入扫描说明"
OVERRIDE_MIN_CHARS = 20


@dataclass(frozen=True)
class Finding:
    rule: str
    severity: str  # "block" | "warn"
    path: str
    line: int
    detail: str


def parse_added_lines(diff: str) -> list[tuple[str, int, str]]:
    """从 unified=0 的 diff 中取出 (文件, 新文件行号, 行内容)。"""
    out: list[tuple[str, int, str]] = []
    path = ""
    lineno = 0
    for raw in diff.splitlines():
        if raw.startswith("+++ "):
            target = raw[4:]
            path = "" if target == "/dev/null" else target.removeprefix("b/")
        elif raw.startswith("@@"):
            m = re.search(r"\+(\d+)", raw)
            lineno = int(m.group(1)) if m else 0
        elif raw.startswith("+") and not raw.startswith("+++") and path:
            out.append((path, lineno, raw[1:]))
            lineno += 1
    return out


def _any(path: str, patterns: list[str]) -> bool:
    return any(path_matches(path, p) for p in patterns)


def scan_line(path: str, lineno: int, text: str) -> list[Finding]:
    found: list[Finding] = []

    if HIDDEN_CHARS.search(text):
        codes = sorted({f"U+{ord(c):04X}" for c in HIDDEN_CHARS.findall(text)})
        found.append(Finding("hidden-unicode", "block", path, lineno,
                             f"含不可见字符 {', '.join(codes)}"))

    if path.startswith(".github/workflows/") and UNTRUSTED_EXPR.search(text) \
            and not ENV_ASSIGNMENT.match(text):
        found.append(Finding("workflow-untrusted-expr", "block", path, lineno,
                             "外部可控内容直接出现在非环境变量位置，应先赋给 env 再引用"))

    if not _any(path, PHRASE_EXEMPT):
        is_doc = _any(path, DOC_PATHS) and not _any(path, INSTRUCTION_FILES)
        for name, pattern in INJECTION_PHRASES:
            if pattern.search(text):
                found.append(Finding("injection-phrase", "warn" if is_doc else "block",
                                     path, lineno, f"命中短语规则：{name}"))
        if path.endswith(".md") and HTML_COMMENT.search(text):
            found.append(Finding("html-comment", "warn", path, lineno,
                                 "Markdown 中的较长 HTML 注释渲染后不可见，请确认内容"))
    return found


def scan_diff(diff: str) -> list[Finding]:
    findings: list[Finding] = []
    for path, lineno, text in parse_added_lines(diff):
        findings.extend(scan_line(path, lineno, text))
    return findings


def override_active(body: str) -> bool:
    section = pr_section(body, OVERRIDE_HEADING)
    return section is not None and len(section) >= OVERRIDE_MIN_CHARS


def decide(findings: list[Finding], body: str) -> tuple[int, list[Finding]]:
    """返回 (退出码, 最终严重度调整后的结果)。

    只有 injection-phrase 的阻断可以被说明降级；隐藏字符和工作流注入不可降级。
    """
    allow = override_active(body)
    final: list[Finding] = []
    for f in findings:
        if allow and f.severity == "block" and f.rule == "injection-phrase":
            f = Finding(f.rule, "warn", f.path, f.line, f.detail + "（已有 PR 说明，降为警告）")
        final.append(f)
    code = 1 if any(f.severity == "block" for f in final) else 0
    return code, final


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--base")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--json-out")
    args = ap.parse_args(argv)
    try:
        base = resolve_base(args.base)
        diff = diff_text(base, args.head)
    except RuntimeError as e:
        print(f"错误：{e}", file=sys.stderr)
        return 2

    code, findings = decide(scan_diff(diff), os.environ.get("PR_BODY", ""))
    for f in findings:
        mark = "阻断" if f.severity == "block" else "警告"
        print(f"[{mark}] {f.path}:{f.line} {f.rule}：{f.detail}")
    print(f"提示注入扫描：{len(findings)} 条发现，结论 {'阻断' if code else '通过'}。")
    if args.json_out:
        Path(args.json_out).write_text(
            json.dumps([asdict(f) for f in findings], ensure_ascii=False, indent=2),
            encoding="utf-8")
    return code


if __name__ == "__main__":
    sys.exit(main())
