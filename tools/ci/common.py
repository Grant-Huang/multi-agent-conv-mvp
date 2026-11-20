"""CI 策略脚本的公共工具：读取 git 差异、匹配路径、解析 PR 描述。

只使用 Python 标准库（Python >= 3.11），不需要额外安装依赖，便于在 CI 与本地运行。
"""

from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import PurePosixPath

# 这些文件由工具生成或是锁文件：行数没有审查意义，统计改动规模时排除。
GENERATED_PATTERNS = [
    "web/src/bindings/*",          # cargo test -p an-core 生成的 TypeScript 类型
    "Cargo.lock",
    "web/package-lock.json",
    "tools/websearch/uv.lock",
    "*.lock",
    "evals/results/*",
]


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """把仓库风格的 glob 转为正则。

    规则（与 .gitignore 接近，但更简单，便于审查）：
    - `**/` 匹配零个或多个目录；`/**` 结尾匹配目录下的所有文件；
    - `*` 匹配单个路径段内的任意字符，不跨越 `/`；
    - `?` 匹配单个非 `/` 字符。
    """
    out = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("/**", i) and i + 3 == len(pattern):
            out.append("/.*")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.compile("^" + "".join(out) + "$")


def path_matches(path: str, pattern: str) -> bool:
    return glob_to_regex(pattern).match(path) is not None


def is_generated(path: str) -> bool:
    """是否为生成文件或锁文件（统计改动规模时排除）。"""
    return any(path_matches(path, p) for p in GENERATED_PATTERNS)


def run_git(args: list[str], cwd: str | None = None) -> str:
    result = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} 失败：{result.stderr.strip()}")
    return result.stdout


def resolve_base(explicit: str | None = None) -> str:
    """确定比较基线：命令行参数 > GitHub 的目标分支 > origin/master。"""
    if explicit:
        return explicit
    ref = os.environ.get("GITHUB_BASE_REF")
    if ref:
        return f"origin/{ref}"
    return "origin/master"


@dataclass(frozen=True)
class FileChange:
    path: str
    added: int
    deleted: int
    binary: bool


def expand_diff_paths(path: str) -> list[str]:
    """把 git 可能给出的重命名路径拆成旧路径与新路径。

    即使调用方已加 `--no-renames`，这里仍作兜底：若出现 `old => new` 或
    `dir/{old => new}`，两边路径都返回，风险分级取较高者，避免把迁入 L3
    目录的文件误判成 L0（见 docs/review/2026-11-06-ci-fences.md H2）。
    """
    path = path.strip()
    if " => " not in path:
        return [path]
    # git 花括号重命名：tools/{ci => lib}/common.py
    m = re.match(r"^(.*)\{(.*) => (.*)\}(.*)$", path)
    if m:
        prefix, old, new, suffix = m.groups()
        return [f"{prefix}{old}{suffix}", f"{prefix}{new}{suffix}"]
    old, new = path.split(" => ", 1)
    return [old, new]


def changed_files(base: str, head: str = "HEAD", cwd: str | None = None) -> list[FileChange]:
    """`base...head` 之间的改动（三点语法：只看 head 相对于分叉点的改动）。

    使用 `--no-renames`：重命名显示为删除旧路径 + 新增新路径，两边都参与
    风险分级与规模统计，避免「文档改工作流」这类迁入被判成 L0。
    """
    out = run_git(["diff", "--numstat", "--no-renames", f"{base}...{head}"], cwd=cwd)
    changes: list[FileChange] = []
    for line in out.splitlines():
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        a, d, path = parts
        binary = a == "-" or d == "-"
        added = 0 if binary else int(a)
        deleted = 0 if binary else int(d)
        for p in expand_diff_paths(path):
            changes.append(FileChange(p, added, deleted, binary))
    return changes


def diff_text(
    base: str, head: str = "HEAD", paths: list[str] | None = None, cwd: str | None = None
) -> str:
    """`base...head` 的统一 diff（unified=0，只含改动行，便于解析新增行）。"""
    args = ["diff", "--unified=0", "--no-renames", f"{base}...{head}"]
    if paths:
        args += ["--", *paths]
    return run_git(args, cwd=cwd)


def component_of(path: str) -> str:
    """把路径归到"组件"：crates/<名字>、web、tools/<名字>、docs、evals、.github 等。"""
    p = PurePosixPath(path)
    parts = p.parts
    if not parts:
        return "(root)"
    if parts[0] in ("crates", "tools") and len(parts) > 1:
        return f"{parts[0]}/{parts[1]}"
    if parts[0] == ".github":
        return ".github"
    if len(parts) == 1:
        # 仓库根目录的文件（README.md、Cargo.toml 等）归为一个组件。
        return "(root)"
    return parts[0]


def pr_section(body: str, heading: str) -> str | None:
    """取 PR 描述中某个 `## 标题` 下的正文（到下一个同级标题为止）；没有则返回 None。"""
    if not body:
        return None
    pattern = re.compile(
        rf"^##\s*{re.escape(heading)}\s*$(.*?)(?=^##\s|\Z)", re.MULTILINE | re.DOTALL
    )
    m = pattern.search(body)
    if not m:
        return None
    text = m.group(1).strip()
    return text or None
