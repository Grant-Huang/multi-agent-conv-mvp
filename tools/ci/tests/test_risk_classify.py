"""risk_classify.py 的测试：策略加载校验、路径分级、内容信号、真实 git 端到端。"""

import subprocess
import sys
from pathlib import Path

import pytest

import risk_classify as rc
from tests.conftest import commit_all

REPO_ROOT = Path(__file__).resolve().parents[3]
REAL_POLICY = REPO_ROOT / "config" / "risk-policy.toml"

# 测试用的小策略：避免测试依赖真实策略文件的具体内容。
POLICY = {
    "default_level": "L2",
    "approvals": {"L0": "a", "L1": "b", "L2": "c", "L3": "d"},
    "rules": [
        {"level": "L0", "paths": ["docs/**", "**/*.md"], "reason": "文档"},
        {"level": "L1", "paths": ["web/**"], "reason": "前端"},
        {"level": "L3", "paths": [".github/**", "AGENTS.md"], "reason": "治理"},
    ],
    "signals": [
        {
            "name": "数据库结构",
            "level": "L2",
            "paths": ["store.rs"],
            "pattern": r"(?i)\bCREATE\s+TABLE\b",
        }
    ],
}


def test_unmatched_file_uses_default_level_not_ignored():
    # 没登记的新目录不能被忽略，必须落到默认等级。
    risk = rc.classify_path("brand/new/file.rs", POLICY["rules"], "L2")
    assert risk.level == "L2"
    assert "默认等级" in risk.reasons[0]


def test_highest_matching_rule_wins():
    # AGENTS.md 同时匹配 **/*.md (L0) 和 AGENTS.md (L3)，应取 L3。
    risk = rc.classify_path("AGENTS.md", POLICY["rules"], "L2")
    assert risk.level == "L3"


def test_overall_level_is_max_over_files():
    result = rc.evaluate(["docs/a.md", "web/x.ts"], {}, POLICY)
    assert result["level"] == "L1"
    assert result["approval"] == "b"


def test_no_changes_yields_default_level():
    # 没有改动时不应给出 L0 的"自动合并"结论，而是默认等级。
    assert rc.evaluate([], {}, POLICY)["level"] == "L2"


def test_signal_only_checks_added_lines_and_raises_level():
    # 删除的 CREATE TABLE 不应触发信号；新增的才触发。
    doc_only = {"docs/a.md": ["CREATE TABLE x"]}
    assert rc.evaluate(["docs/a.md"], doc_only, POLICY)["level"] == "L0"

    added = {"store.rs": ["CREATE TABLE IF NOT EXISTS t (id INT)"]}
    result = rc.evaluate(["store.rs"], added, POLICY)
    assert result["level"] == "L2"
    assert any("数据库结构" in r for r in result["files"][0]["reasons"])


def test_signal_cannot_lower_level():
    # 信号只能提高等级：L3 文件即使命中 L2 信号，仍是 L3。
    policy = {**POLICY, "signals": [{"name": "x", "level": "L2", "pattern": "a"}]}
    result = rc.evaluate([".github/workflows/ci.yml"], {".github/workflows/ci.yml": ["a"]}, policy)
    assert result["level"] == "L3"


def test_added_lines_of_parses_unified0_diff():
    diff = (
        "diff --git a/x.rs b/x.rs\n"
        "--- a/x.rs\n"
        "+++ b/x.rs\n"
        "@@ -1 +1,2 @@\n"
        "-old\n"
        "+new\n"
        "+++ not a header but content\n"
        "diff --git a/gone.rs b/gone.rs\n"
        "--- a/gone.rs\n"
        "+++ /dev/null\n"
        "@@ -1 +0,0 @@\n"
        "-bye\n"
    )
    added = rc.added_lines_of(diff)
    assert added["x.rs"] == ["new", "++ not a header but content"]
    assert "gone.rs" not in added


def test_load_policy_rejects_bad_level(tmp_path):
    p = tmp_path / "p.toml"
    p.write_text('default_level = "L9"\n', encoding="utf-8")
    with pytest.raises(rc.PolicyError):
        rc.load_policy(p)


def test_load_policy_rejects_bad_regex(tmp_path):
    p = tmp_path / "p.toml"
    p.write_text('[[signals]]\nname="x"\nlevel="L2"\npattern="("\n', encoding="utf-8")
    with pytest.raises(rc.PolicyError):
        rc.load_policy(p)


def test_real_policy_file_loads():
    # 仓库自带的策略必须能通过校验，否则 CI 会在策略加载阶段失败。
    policy = rc.load_policy(REAL_POLICY)
    assert policy["default_level"] == "L2"


def test_end_to_end_on_real_repo(repo):
    # 在真实 git 仓库中运行 CLI：新增一个 store.rs 并写入建表语句。
    (repo / "store.rs").write_text("CREATE TABLE t (id INT);\n", encoding="utf-8")
    policy = repo / "policy.toml"
    policy.write_text(
        'default_level = "L0"\n[[signals]]\nname="表"\nlevel="L2"\n'
        'paths=["store.rs"]\npattern="CREATE TABLE"\n',
        encoding="utf-8",
    )
    commit_all(repo)
    out = repo / "risk.json"
    script = Path(rc.__file__)
    proc = subprocess.run(
        [sys.executable, str(script), "--base", "master", "--head", "feature",
         "--policy", str(policy), "--json-out", str(out)],
        cwd=repo, capture_output=True, text=True, check=False,
    )
    assert proc.returncode == 0, proc.stderr
    import json

    data = json.loads(out.read_text(encoding="utf-8"))
    assert data["level"] == "L2"


def test_cli_returns_2_on_missing_policy(repo):
    proc = subprocess.run(
        [sys.executable, str(Path(rc.__file__)), "--base", "master",
         "--policy", str(repo / "missing.toml")],
        cwd=repo, capture_output=True, text=True, check=False,
    )
    assert proc.returncode == 2
