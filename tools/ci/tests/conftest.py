"""测试公共夹具：创建临时 git 仓库，用于验证真实 git 输出的解析。"""

import subprocess
from pathlib import Path

import pytest


def git(cwd: Path, *args: str) -> str:
    """在 cwd 中执行 git 命令；失败时抛出异常，便于定位测试问题。"""
    result = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=True
    )
    return result.stdout


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    """初始化一个仓库，提交 base 版本，并切到 feature 分支。

    返回仓库路径；测试中可用 `base=master` 与 `head=feature` 比较。
    """
    git(tmp_path, "init", "-q", "-b", "master")
    git(tmp_path, "config", "user.email", "test@example.com")
    git(tmp_path, "config", "user.name", "test")
    (tmp_path / "keep.txt").write_text("a\nb\n", encoding="utf-8")
    git(tmp_path, "add", ".")
    git(tmp_path, "commit", "-q", "-m", "base")
    git(tmp_path, "checkout", "-q", "-b", "feature")
    return tmp_path


def commit_all(repo: Path, msg: str = "change") -> None:
    """暂存并提交所有改动。"""
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", msg)
