"""common.py 的测试：glob 匹配、路径分组、PR 描述解析、真实 git 输出解析。"""

import pytest

from common import (
    FileChange,
    changed_files,
    component_of,
    diff_text,
    expand_diff_paths,
    is_generated,
    path_matches,
    pr_section,
    resolve_base,
)
from tests.conftest import commit_all, git


@pytest.mark.parametrize(
    "path, pattern, expected",
    [
        ("web/src/a.ts", "web/**", True),
        ("web/src/bindings/X.ts", "web/src/bindings/*", True),
        ("web/src/bindings/sub/X.ts", "web/src/bindings/*", False),  # * 不跨目录
        ("Cargo.lock", "Cargo.lock", True),
        ("tools/websearch/uv.lock", "**/*.lock", True),
        ("crates/a/Cargo.toml", "**/Cargo.toml", True),
        ("Cargo.toml", "**/Cargo.toml", True),  # ** 匹配零层目录
        ("AGENTS.md", "**/*.md", True),
        ("docs/a/b.md", "**/*.md", True),
        ("a.rs", "a.r?", True),
        ("a/b.rs", "a?b.rs", False),  # ? 不匹配 /
        ("crates/x.rs", "crates/x.rs.bak", False),
        ("a.b", "a+b", False),  # 正则元字符必须被转义
        ("a+b", "a+b", True),
    ],
)
def test_path_matches(path, pattern, expected):
    assert path_matches(path, pattern) is expected


def test_is_generated_covers_lock_and_bindings():
    assert is_generated("web/src/bindings/Foo.ts")
    assert is_generated("evals/results/run.json")
    assert not is_generated("crates/runtime/src/store.rs")


@pytest.mark.parametrize(
    "path, component",
    [
        ("crates/runtime/src/store.rs", "crates/runtime"),
        ("tools/ci/common.py", "tools/ci"),
        (".github/workflows/ci.yml", ".github"),
        ("web/src/App.tsx", "web"),
        ("README.md", "(root)"),
        ("Cargo.toml", "(root)"),
    ],
)
def test_component_of(path, component):
    assert component_of(path) == component


def test_resolve_base_priority(monkeypatch):
    monkeypatch.delenv("GITHUB_BASE_REF", raising=False)
    assert resolve_base() == "origin/master"
    monkeypatch.setenv("GITHUB_BASE_REF", "dev")
    assert resolve_base() == "origin/dev"
    assert resolve_base("abc123") == "abc123"  # 命令行参数优先


def test_pr_section_extracts_until_next_heading():
    body = "## 背景\n说明\n\n## 大改动说明\n这是一段足够长的理由，说明为什么不能拆分。\n## 其他\nx"
    assert pr_section(body, "大改动说明") == "这是一段足够长的理由，说明为什么不能拆分。"
    assert pr_section(body, "不存在") is None
    assert pr_section("", "大改动说明") is None
    assert pr_section("## 测试删除说明\n\n", "测试删除说明") is None  # 空白视为没有


def test_changed_files_and_diff_on_real_repo(repo):
    # 在真实 git 仓库里验证 numstat 解析：增、删、二进制、重命名后的路径。
    (repo / "keep.txt").write_text("a\nc\nd\n", encoding="utf-8")  # 删 1 行 b，增 2 行
    (repo / "new.bin").write_bytes(bytes(range(256)))
    commit_all(repo)

    changes = {c.path: c for c in changed_files("master", "feature", cwd=str(repo))}
    assert changes["keep.txt"] == FileChange("keep.txt", 2, 1, False)
    assert changes["new.bin"].binary is True
    assert changes["new.bin"].added == 0

    text = diff_text("master", "feature", cwd=str(repo))
    assert "+d" in text and "-b" in text


def test_changed_files_bad_ref_raises(repo):
    with pytest.raises(RuntimeError):
        changed_files("no-such-ref", "feature", cwd=str(repo))


@pytest.mark.parametrize(
    "raw, expected",
    [
        ("docs/a.md", ["docs/a.md"]),
        ("docs/tmp/notes.md => .github/workflows/notes.yml",
         ["docs/tmp/notes.md", ".github/workflows/notes.yml"]),
        ("tools/{ci => lib}/common.py",
         ["tools/ci/common.py", "tools/lib/common.py"]),
    ],
)
def test_expand_diff_paths(raw, expected):
    assert expand_diff_paths(raw) == expected


def test_rename_into_l3_path_is_two_entries_not_one(repo):
    # 重命名在 --no-renames 下应显示为删旧 + 增新，两边路径都进入清单。
    (repo / "docs").mkdir()
    (repo / "docs" / "notes.md").write_text("name: ci\non: push\n", encoding="utf-8")
    commit_all(repo, "add doc")
    git(repo, "checkout", "-q", "-b", "rename")
    (repo / ".github").mkdir()
    (repo / ".github" / "workflows").mkdir(parents=True)
    git(repo, "mv", "docs/notes.md", ".github/workflows/notes.yml")
    commit_all(repo, "rename into workflows")

    paths = {c.path for c in changed_files("feature", "rename", cwd=str(repo))}
    assert "docs/notes.md" in paths
    assert ".github/workflows/notes.yml" in paths
    # 不应再出现带 => 的合成路径。
    assert not any(" => " in p for p in paths)
