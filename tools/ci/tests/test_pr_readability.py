"""pr_readability.py 的测试：以 #429/#413（应告警）与 #450（应通过）的真实形态为夹具。"""

from pathlib import Path

import pr_readability as pr

REPO_ROOT = Path(__file__).resolve().parents[3]
GLOSSARY = str(REPO_ROOT / "docs" / "GLOSSARY.md")

BODY_429 = """## 变更说明
<!-- 解决什么问题、改了什么、为什么这样改。 -->

## 关联
- Case / Feature ID：
"""

BODY_413 = """[第5线] feat(convmem): #346 片5 写入面扫描 flags + 撤回影响追溯（Closes #346）

## 变更说明
#346 收尾片（REQ-conversation-memory-07，2026-11-26 用户确认冻结）：
1. 写入面扫描（§7.3）：memory_json 读时派生 flags，只标注不阻断。
2. 撤回影响追溯（§7.6 / CH-G3）：新表记录撤回影响的任务。
"""

BODY_450 = """## 给审查人

- **做了什么**：新建 docs/GLOSSARY.md，把仓库里常见的缩写、编号前缀、内部术语都用一句白话解释。
- **为什么**：你反映 PR 里很多缩写不是你定义的，对齐上下文很花时间。
- **需要你做什么**：无（L0，CI 绿后由审查会话合并）。
"""


def _check(title, body, risk=None, glossary=None):
    return pr.check(title, body, glossary_path=glossary or GLOSSARY, risk_level=risk)


def test_429_style_branchy_title_warns():
    # 卡面正例：#429 标题 "Line1 uxfix k 314" 应告警（格式 + 分支样式 + 缺给审查人）。
    ws = _check("Line1 uxfix k 314", BODY_429)
    assert any("标题格式" in w for w in ws)
    assert any("分支名" in w for w in ws)
    assert any("给审查人" in w for w in ws)


def test_413_warns_missing_level_and_reviewer():
    # 卡面正例：#413 有 [第5线] 但缺 [Lx] 风险段，正文无「给审查人」。
    ws = _check(
        "[第5线] feat(convmem): #346 片5 写入面扫描 flags + 撤回影响追溯（Closes #346）",
        BODY_413,
    )
    assert any("[L0~L3]" in w for w in ws)
    assert any("给审查人" in w for w in ws)
    # REQ- 已登记：不得误报缩写
    assert not any("REQ" in w for w in ws)


def test_450_passes_clean():
    # 卡面反例：#450 格式齐全、给审查人 3 行、无未登记缩写、风险一致 → 零警告。
    ws = _check(
        "[审查][L0] 新建术语表，PR 里的缩写和编号有地方可查",
        BODY_450,
        risk="L0",
        glossary=GLOSSARY,
    )
    assert ws == [], ws


def test_risk_mismatch_warns():
    ws = _check("[第2线][L0] 某修复", BODY_450, risk="L3", glossary=GLOSSARY)
    assert any("风险等级不一致" in w for w in ws)


def test_unregistered_acronym_listed():
    ws = _check("[第2线][L1] 接入 XYZABC 服务", BODY_450, glossary=GLOSSARY)
    assert any("XYZABC" in w for w in ws)


def test_builtin_acronym_not_warned():
    ws = _check("[第2线][L1] 修复 MCP 工具的 HTTP 超时", BODY_450, glossary=GLOSSARY)
    assert not any("未登记缩写" in w for w in ws), ws


def test_reviewer_section_over_three_lines_warns():
    body = "## 给审查人\n\n- 一\n- 二\n- 三\n- 四\n"
    ws = _check("[审查][L0] 标题", body, glossary=GLOSSARY)
    assert any("超过 3 行" in w for w in ws)
