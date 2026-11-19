"""change_size_check.py 的测试：阈值、覆盖说明、生成文件排除、测试删除检测。"""

import subprocess
import sys
from pathlib import Path

import change_size_check as cs
from common import FileChange
from tests.conftest import commit_all

LONG_SIZE_NOTE = (
    "## 大改动说明\n"
    "这次改动是一次性迁移，拆分会导致中间状态无法编译，"
    "因此必须整体审查，请审查人确认迁移的完整性。\n"
)
LONG_TEST_NOTE = "## 测试删除说明\n旧测试覆盖的功能已被删除，对应测试一并删除。\n"
# 满足「非目标 + Case ID」门槛，避免模板警告干扰规模/测试类断言。
META = (
    "## 非目标\n不修改任务状态机、不改公开 HTTP API。\n"
    "## 关联\nCase ID：RT-FENCE-01\n"
)
LONG_WEAKEN_NOTE = (
    "## 测试变更说明\n"
    "旧断言覆盖的路径已删除，对应 assert 一并去掉，并补充新路径的用例。\n"
)


def fc(path, added, deleted=0):
    return FileChange(path, added, deleted, False)


def test_small_change_passes():
    r = cs.evaluate([fc("crates/runtime/src/a.rs", 100, 20)], META)
    assert r["status"] == "pass"
    assert r["lines"] == 120


def test_warn_threshold_lines_and_files():
    assert cs.evaluate([fc("crates/a/x.rs", 800)], META)["status"] == "warn"
    many = [fc(f"crates/a/f{i}.rs", 1) for i in range(20)]
    assert cs.evaluate(many, META)["status"] == "warn"


def test_block_threshold_without_note_blocks():
    assert cs.evaluate([fc("crates/a/x.rs", 2000)], META)["status"] == "block"
    many = [fc(f"crates/a/f{i}.rs", 1) for i in range(40)]
    assert cs.evaluate(many, META)["status"] == "block"


def test_block_threshold_with_note_downgrades_to_warn():
    r = cs.evaluate([fc("crates/a/x.rs", 2500)], LONG_SIZE_NOTE + META)
    assert r["status"] == "warn"
    assert any("已提供" in f["message"] for f in r["findings"])


def test_short_size_note_is_not_enough():
    # 说明少于 40 字不算数，避免用一个字糊弄过去。
    short = "## 大改动说明\n需要\n" + META
    assert cs.evaluate([fc("crates/a/x.rs", 2500)], short)["status"] == "block"


def test_generated_files_are_excluded_from_counts():
    # 锁文件与生成的 TS 绑定不计入行数，否则依赖升级会被误判为大改动。
    changes = [
        fc("Cargo.lock", 5000),
        fc("web/src/bindings/Big.ts", 3000),
        fc("crates/a/x.rs", 10),
    ]
    r = cs.evaluate(changes, META)
    assert r["status"] == "pass"
    assert r["lines"] == 10
    assert r["files"] == 1


def test_component_count_warns_but_docs_and_evals_exempt():
    changes = [fc(f"crates/c{i}/x.rs", 1) for i in range(4)]
    r = cs.evaluate(changes, META)
    assert r["status"] == "warn"
    assert any("4 个组件" in f["message"] for f in r["findings"])

    # docs 和 evals 不算组件，不应触发组件数警告。
    docs = [fc("crates/a/x.rs", 1), fc("docs/a.md", 1), fc("evals/b.jsonl", 1),
            fc("web/x.ts", 1), fc("tools/ci/y.py", 1)]
    assert cs.evaluate(docs, META)["status"] == "pass"


def test_removed_tests_block_unless_noted():
    r = cs.evaluate([fc("crates/a/x.rs", 1)], META, removed_tests=3, added_tests=1)
    assert r["status"] == "block"
    r = cs.evaluate(
        [fc("crates/a/x.rs", 1)], LONG_TEST_NOTE + META, removed_tests=3, added_tests=1
    )
    assert r["status"] == "warn"


def test_equal_test_counts_pass():
    # 只是改名或重写测试、数量不减少时，不应阻断。
    r = cs.evaluate([fc("crates/a/x.rs", 1)], META, removed_tests=2, added_tests=2)
    assert r["status"] == "pass"


def test_weaken_signals_block_unless_noted():
    r = cs.evaluate(
        [fc("crates/a/x.rs", 1)], META, weaken_added=1, assert_added=0, assert_removed=0
    )
    assert r["status"] == "block"
    r = cs.evaluate(
        [fc("crates/a/x.rs", 1)],
        META,
        weaken_added=0,
        assert_added=0,
        assert_removed=2,
    )
    assert r["status"] == "block"
    r = cs.evaluate(
        [fc("crates/a/x.rs", 1)],
        LONG_WEAKEN_NOTE + META,
        weaken_added=1,
        assert_added=0,
        assert_removed=2,
    )
    assert r["status"] == "warn"


def test_missing_scope_or_case_id_warns_for_code_changes():
    r = cs.evaluate([fc("crates/a/x.rs", 1)], None)
    assert r["status"] == "warn"
    msgs = " ".join(f["message"] for f in r["findings"])
    assert "非目标" in msgs
    assert "Case/Feature" in msgs
    # 纯文档不要求。
    assert cs.evaluate([fc("docs/a.md", 5)], None)["status"] == "pass"


def test_is_test_path():
    assert cs.is_test_path("tools/ci/tests/test_change_size_check.py")
    assert cs.is_test_path("crates/runtime/tests/mom_des.rs")
    assert cs.is_test_path("web/src/ui.test.tsx")
    assert not cs.is_test_path("tools/ci/change_size_check.py")
    assert not cs.is_test_path("docs/fencing/EVAL_LOOP.md")


def test_count_weaken_signals_ignores_non_test_paths():
    # 策略脚本自身新增含 `.skip(` 的正则，不应计为测试弱化。
    diff = (
        "diff --git a/tools/ci/change_size_check.py b/tools/ci/change_size_check.py\n"
        "--- a/tools/ci/change_size_check.py\n"
        "+++ b/tools/ci/change_size_check.py\n"
        "@@ -1 +1,2 @@\n"
        "+WEAKEN = re.compile(r'.skip(')\n"
        "diff --git a/tools/ci/tests/test_x.py b/tools/ci/tests/test_x.py\n"
        "--- a/tools/ci/tests/test_x.py\n"
        "+++ b/tools/ci/tests/test_x.py\n"
        "@@ -1 +1,2 @@\n"
        "+    it.skip('x', () => {})\n"
    )
    weaken, a_add, a_del = cs.count_weaken_signals(diff)
    assert weaken == 1
    assert a_add == 0 and a_del == 0


def test_count_test_markers_ignores_diff_headers():
    diff = (
        "diff --git a/crates/x.rs b/crates/x.rs\n"
        "--- a/crates/x.rs\n"
        "+++ b/crates/x.rs\n"
        "@@ -1,2 +1,2 @@\n"
        "-#[test]\n"
        "-fn old() {}\n"
        "+#[tokio::test]\n"
        "+async fn new() {}\n"
        "-def test_gone():\n"
        "+    it('ok', () => {})\n"
    )
    added, removed = cs.count_test_markers(diff)
    assert (added, removed) == (2, 2)


def test_end_to_end_on_real_repo(repo):
    # 真实 git 仓库：删除一个 Rust 测试，且没有说明，应当阻断（退出码 1）。
    (repo / "t.rs").write_text("#[test]\nfn a() {}\n#[test]\nfn b() {}\n", encoding="utf-8")
    commit_all(repo, "add tests")
    git = ["git", "-C", str(repo)]
    subprocess.run([*git, "checkout", "-q", "-b", "feature2"], check=True)
    (repo / "t.rs").write_text("#[test]\nfn a() {}\n", encoding="utf-8")
    commit_all(repo, "drop test")

    script = str(Path(cs.__file__))
    proc = subprocess.run(
        [sys.executable, script, "--base", "feature", "--head", "feature2"],
        cwd=repo, capture_output=True, text=True, check=False,
        env={"PATH": "/usr/bin:/bin", "PR_BODY": ""},
    )
    assert proc.returncode == 1, proc.stdout + proc.stderr
    assert "删除的测试标记" in proc.stdout
