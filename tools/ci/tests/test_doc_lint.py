"""doc_lint.py 的测试：每条文档规则的命中与放行，以及真实 git 端到端（只警告、不阻断）。

用例编号对应 docs/DOC_SYSTEM_PLAN.md 第 4.3 节的检查项。
"""

import json
import os
import subprocess
import sys
from pathlib import Path

import doc_lint as lint
from tests.conftest import commit_all

TOOL = Path(__file__).resolve().parents[1] / "doc_lint.py"

GOOD_DECISION = """# 一句话标题

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-22 |
| 来源 | 用户确认 |
| 状态 | 有效 |
| 影响范围 | docs |
| 相关设计 | 无 |

**一句话结论**：……

## 背景
……

## 问题
……

## 决策依据
1. ……

## 定了什么
- ……

## 没选什么
| 做法 | 为什么没选 |
|---|---|

## 出问题先看
- ……
"""


def rules(findings):
    return sorted(f.rule for f in findings)


# ---------- 锚点规则（GitHub 风格） ----------

def test_github_slug_keeps_chinese_and_drops_punctuation():
    assert lint.github_slug("0. 系统全景") == "0-系统全景"
    assert lint.github_slug("0.3 产品特性设计索引") == "03-产品特性设计索引"
    assert lint.github_slug("Hello, World!") == "hello-world"


def test_heading_anchors_ignore_code_blocks_and_number_duplicates():
    text = "# 标题\n```\n# 不是标题\n```\n## 标题\n"
    assert lint.heading_anchors(text) == {"标题", "标题-1"}


# ---------- DOC-LINK：新增行里的相对链接与锚点 ----------

def test_broken_relative_link_in_added_line_warns(tmp_path):
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "a.md").write_text("[x](missing.md)\n", encoding="utf-8")
    found = lint.check_added_links([("docs/a.md", 1, "[x](missing.md)")], tmp_path)
    assert rules(found) == ["broken-link"]
    assert found[0].line == 1


def test_missing_anchor_warns_and_existing_anchor_passes(tmp_path):
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "b.md").write_text("# 系统全景\n", encoding="utf-8")
    bad = lint.check_added_links([("docs/a.md", 3, "[x](b.md#不存在)")], tmp_path)
    good = lint.check_added_links([("docs/a.md", 3, "[x](b.md#系统全景)")], tmp_path)
    assert rules(bad) == ["broken-link"]
    assert good == []


def test_external_links_and_non_markdown_files_are_ignored(tmp_path):
    added = [
        ("docs/a.md", 1, "[x](https://example.com/a.md)"),
        ("docs/a.md", 2, "[y](mailto:a@example.com)"),
        ("crates/x.rs", 1, "// [z](missing.md)"),
    ]
    assert lint.check_added_links(added, tmp_path) == []


def test_link_to_anchor_in_same_file(tmp_path):
    (tmp_path / "a.md").write_text("# 一\n[x](#一)\n[y](#二)\n", encoding="utf-8")
    found = lint.check_added_links(
        [("a.md", 2, "[x](#一)"), ("a.md", 3, "[y](#二)")], tmp_path)
    assert [f.line for f in found] == [3]


# ---------- DOC-DECISION：docs/decisions/ 新文件的格式 ----------

def test_well_formed_decision_passes():
    assert lint.check_decision_file("docs/decisions/2026-11-22-good-one.md", GOOD_DECISION) == []


def test_bad_decision_file_name_warns():
    found = lint.check_decision_file("docs/decisions/第70条.md", GOOD_DECISION)
    assert rules(found) == ["decision-name"]


def test_decision_missing_header_fields_and_sections_warns():
    text = "# 标题\n\n| 项目 | 内容 |\n|---|---|\n| 日期 | 2026-11-22 |\n\n## 问题\n……\n"
    found = lint.check_decision_file("docs/decisions/2026-11-22-thin.md", text)
    details = " ".join(f.detail for f in found)
    assert set(rules(found)) == {"decision-header", "decision-section"}
    for word in ("来源", "状态", "影响范围", "相关设计", "背景", "决策依据", "没选什么"):
        assert word in details


def test_decisions_readme_is_not_checked_as_decision():
    assert lint.check_decision_file("docs/decisions/README.md", "# 说明\n") == []


# ---------- DOC-APPEND：不再往 DECISIONS.md 追加编号条目 ----------

def test_appending_numbered_entry_to_decisions_md_warns():
    head = "### 1. 旧\n### 70. 新增\n"
    found = lint.check_decisions_append([("docs/DECISIONS.md", 2, "### 70. 新增")], head)
    assert rules(found) == ["decisions-append"]


def test_duplicate_number_is_reported_in_detail():
    head = "### 69. 旧的 69\n### 69. 又一个 69\n"
    found = lint.check_decisions_append([("docs/DECISIONS.md", 2, "### 69. 又一个 69")], head)
    assert rules(found) == ["decisions-append"]
    assert "重复" in found[0].detail


def test_annotations_in_decisions_md_are_allowed():
    added = [("docs/DECISIONS.md", 5, "> 编号说明：本条引用时写作「61（AI 桌面）」"),
             ("docs/other.md", 1, "### 3. 普通文档标题")]
    assert lint.check_decisions_append(added, "### 61. x\n") == []


# ---------- DOC-INDEX：新增设计文档须在 DESIGN 第 0.3 节登记 ----------

def test_unregistered_new_design_doc_warns():
    index = "| x | [mom/DESIGN.md](mom/DESIGN.md) |"
    found = lint.check_design_registered(["docs/newthing/DESIGN.md"], index)
    assert rules(found) == ["design-unregistered"]


def test_registered_design_doc_and_non_design_docs_pass():
    index = "| x | [newthing/DESIGN.md](newthing/DESIGN.md) |"
    new = ["docs/newthing/DESIGN.md", "docs/testing/evidence/x.md", "docs/manual/a.md"]
    assert lint.check_design_registered(new, index) == []


def test_new_feature_dossier_must_be_registered():
    found = lint.check_design_registered(["docs/features/world-adapter/README.md"], "")
    assert rules(found) == ["design-unregistered"]


# ---------- DOC-PR：PR 正文的“设计一致性”一节 ----------

def test_pr_without_design_consistency_section_warns():
    found = lint.check_pr_body("## 变更说明\n改了代码\n", ["crates/x.rs"])
    assert rules(found) == ["pr-design-consistency"]


def test_pr_with_section_passes_and_test_only_pr_is_exempt():
    body = "## 设计一致性\n- 属于哪个特性：conversation\n"
    assert lint.check_pr_body(body, ["crates/x.rs"]) == []
    assert lint.check_pr_body("", ["crates/x/tests/a.rs", "tools/ci/tests/test_a.py"]) == []


def test_missing_pr_body_skips_check():
    """本地运行没有 PR_BODY 时不检查（None 表示未提供）。"""
    assert lint.check_pr_body(None, ["crates/x.rs"]) == []


# ---------- 端到端：真实 git，只警告不阻断 ----------

def run_cli(repo: Path, body: str | None = None, *extra: str):
    env = dict(os.environ)
    env.pop("PR_BODY", None)
    if body is not None:
        env["PR_BODY"] = body
    return subprocess.run([sys.executable, str(TOOL), "--base", "master", *extra],
                          cwd=repo, capture_output=True, text=True, env=env)


def test_cli_warns_but_exits_zero(repo):
    (repo / "docs" / "decisions").mkdir(parents=True)
    (repo / "docs" / "decisions" / "bad name.md").write_text("# x\n", encoding="utf-8")
    (repo / "docs" / "a.md").write_text("[x](nowhere.md)\n", encoding="utf-8")
    commit_all(repo, "docs")
    r = run_cli(repo, "## 变更说明\nx\n", "--json-out", "out.json")
    assert r.returncode == 0, r.stdout + r.stderr
    assert "警告" in r.stdout
    data = json.loads((repo / "out.json").read_text(encoding="utf-8"))
    assert {"broken-link", "decision-name"} <= {d["rule"] for d in data}


def test_cli_strict_mode_exits_one_on_findings(repo):
    (repo / "a.md").write_text("[x](nowhere.md)\n", encoding="utf-8")
    commit_all(repo, "docs")
    assert run_cli(repo, None, "--strict").returncode == 1


def test_cli_clean_change_has_no_findings(repo):
    (repo / "a.md").write_text("普通说明\n", encoding="utf-8")
    commit_all(repo, "docs")
    r = run_cli(repo, None)
    assert r.returncode == 0
    assert "0 条" in r.stdout
