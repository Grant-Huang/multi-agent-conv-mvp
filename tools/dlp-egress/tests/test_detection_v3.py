"""识别规则第三轮改进（依据 gold-v3 开发部分的失败类别，见 D-018）。

例子全部为新编写，不取自 gold-v3，用来约束规则的通用性：
- 机密信息的“密级/流转限制”标记（涉密、机密等级、限 X 查看、暂不披露、未经批准不得引用……）
  出现在名词短语开头或中间，短语以“上限、区间、范围、边界、顺序、路线……”收尾；
- 英文的密级标记直接放在名词短语前（need-to-know、controlled-access、embargoed……），
  或短语内带 unannounced / undisclosed / unapproved 等词；
- 谈判底线类概念（最低净价、让步区间、议价授权、lowest acceptable price）本身就是机密；
- 更多生产指标名称（首检通过率、pass rate）。
"""
import pytest

from agentnexus_dlp.detector import Detector


@pytest.fixture(scope="module")
def det():
    return Detector(master_data=None)


def found(det, text):
    return {(e.type, text[e.start:e.end]) for e in det.detect(text, None)}


def has_internal(det, text):
    return any(t == "INTERNAL_INFO" for t, _ in found(det, text))


# ---------------------------------------------------------------- 中文：密级/流转限制标记 + 名词短语
@pytest.mark.parametrize("text, span", [
    # 标记在开头
    ("附件里是涉密的明年渠道返点调整区间，请勿转发", "涉密的明年渠道返点调整区间"),
    ("这里记录了限管理层查看的核心供应商淘汰顺序。", "限管理层查看的核心供应商淘汰顺序"),
    ("纪要提到机密等级的海外仓备货上限", "机密等级的海外仓备货上限"),
    ("请确认禁止外发的设备改造预算上限", "禁止外发的设备改造预算上限"),
    ("对方索要未经批准不得引用的代工产线切换路线", "未经批准不得引用的代工产线切换路线"),
    # 带时间/条件限定语的标记
    ("材料中有上市前暂不披露的新品定价范围", "上市前暂不披露的新品定价范围"),
    # 标记在短语中间（限定语 + 内部/未宣布 + 名词）
    ("草稿写的是旺季缺货时内部认可的交期延误边界", "旺季缺货时内部认可的交期延误边界"),
    ("名单涉及明年合并的未宣布班组安排", "明年合并的未宣布班组安排"),
])
def test_zh_confidential_marker_phrases(det, text, span):
    assert ("INTERNAL_INFO", span) in found(det, text)


# ---------------------------------------------------------------- 英文：密级标记 + 名词短语
@pytest.mark.parametrize("text, span", [
    ("This deck contains the unannounced plant consolidation plan.",
     "the unannounced plant consolidation plan"),
    ("Do not forward the undisclosed warranty settlement range to the vendor.",
     "the undisclosed warranty settlement range"),
    ("The memo covers need-to-know the revised supplier exit sequencing.",
     "need-to-know the revised supplier exit sequencing"),
    ("Section 3 lists controlled-access the regional discount ladder; see below.",
     "controlled-access the regional discount ladder"),
    ("It references the unapproved furnace temperature adjustment.",
     "the unapproved furnace temperature adjustment"),
])
def test_en_confidential_marker_phrases(det, text, span):
    assert ("INTERNAL_INFO", span) in found(det, text)


# ---------------------------------------------------------------- 谈判底线类概念（本身即机密）
@pytest.mark.parametrize("text", [
    "这次招标我们能接受的最低净价已经定了",
    "法务给出的索赔让步区间请勿外传",
    "销售经理的议价授权范围见附件",
    "Our lowest acceptable price for the tender is attached.",
])
def test_negotiation_floor_concepts(det, text):
    assert has_internal(det, text)


# ---------------------------------------------------------------- 负例：公开信息、普通管理用语不应判为机密
@pytest.mark.parametrize("text", [
    "公开发布的产品目录和已公告的停产安排见附件",
    "请按公司通知的放假安排准时返岗",
    "The public price list and the published shutdown schedule are attached.",
    "The announced plant consolidation plan is on our website.",
    "下周举行内部培训，请准时参加",
])
def test_confidential_marker_negatives(det, text):
    assert not has_internal(det, text)


# ---------------------------------------------------------------- 指标
@pytest.mark.parametrize("text, span", [
    ("本班首检通过率97.4%", "首检通过率97.4%"),
    ("出厂检验通过率99.1%，符合要求", "通过率99.1%"),
    ("final test pass rate 98.2% today", "pass rate 98.2%"),
])
def test_pass_rate_metrics(det, text, span):
    assert ("METRIC", span) in found(det, text)


# ---------------------------------------------------------------- 普通词误报（干净样本里过度脱敏）
@pytest.mark.parametrize("text", [
    "请确认材料是否齐全",
    "所有材料已归档",
    "转发材料前请先核对",
    "此段材料仅供参考",
])
def test_generic_material_words_are_not_companies(det, text):
    assert not any(t == "CUSTOMER" for t, _ in found(det, text))


@pytest.mark.parametrize("text", [
    "项目编码请填写在表头",
    "项目代号统一使用拼音",
    "项目成员请于周五前提交周报",
])
def test_generic_project_words_are_not_project_names(det, text):
    assert not any(t == "PROJECT" for t, _ in found(det, text))


def test_real_company_and_project_names_still_detected(det):
    text = "华岳新材料和项目青松2的交付安排"
    got = found(det, text)
    assert ("CUSTOMER", "华岳新材料") in got
    assert ("PROJECT", "项目青松2") in got
