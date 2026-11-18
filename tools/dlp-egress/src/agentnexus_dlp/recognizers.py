"""自定义识别器（全部以 Presidio EntityRecognizer 的形式注册）。

设计原则：
- 这里只放**通用**规则：行业惯用编号前缀、货币/单位格式、公司名后缀、职务称谓、
  商业机密概念词等，不内置任何评测集里的具体客户/项目名。
- 具体的客户、项目、厂区、人员名单属于“租户主数据”，由配置注入（见 MasterDataRecognizer）。
- 中文没有空格，Python re 的 \\b 会把汉字当作单词字符，因此 ASCII 编号一律用
  (?<![A-Za-z0-9]) / (?![A-Za-z0-9]) 作为边界，而不是 \\b。
"""
from __future__ import annotations

import re
from dataclasses import dataclass

from presidio_analyzer import EntityRecognizer, RecognizerResult

# 所有自定义识别器使用的语言代码。中英混排很常见，所以不按语言拆分识别器，
# 统一在多语言模式下运行全部规则。
LANG = "xx"

# ASCII 边界
_L = r"(?<![A-Za-z0-9])"
_R = r"(?![A-Za-z0-9])"
# 汉字前缀中不应出现的虚词/动词（避免 "请将云岭传动" 被整段识别）
_ZH_STOP = "的将在和与及把给向对为从是了请到至于"
_ZH_CHAR = rf"(?:(?![{_ZH_STOP}])[一-鿿])"


@dataclass(frozen=True)
class Rule:
    """一条正则规则。group 指定实体所在的命名组（默认整段）。"""
    entity: str
    pattern: re.Pattern[str]
    score: float = 0.8
    group: str | int = 0


class RegexRecognizer(EntityRecognizer):
    """支持捕获组的正则识别器（Presidio 内置 PatternRecognizer 不支持只取子组）。"""

    def __init__(self, name: str, rules: list[Rule], source: str = "rule"):
        self.rules = rules
        self.source = source
        super().__init__(
            supported_entities=sorted({r.entity for r in rules}),
            name=name, supported_language=LANG,
        )

    def load(self) -> None:  # 规则无需加载模型
        pass

    def analyze(self, text, entities, nlp_artifacts=None):
        results = []
        for rule in self.rules:
            if entities and rule.entity not in entities:
                continue
            for m in rule.pattern.finditer(text):
                start, end = m.span(rule.group)
                if start < 0 or end <= start:
                    continue
                results.append(RecognizerResult(
                    entity_type=rule.entity, start=start, end=end, score=rule.score,
                    recognition_metadata={"recognizer_name": self.name, "source": self.source},
                ))
        return results


# ---------------------------------------------------------------------------
# 凭证（结构化规则；detect-secrets 在 secrets.py 中另行封装）
# ---------------------------------------------------------------------------
SECRET_RULES = [
    # HTTP Authorization 头：Bearer / Basic / Token 后面的值
    Rule("SECRET", re.compile(
        r"(?i)authorization\s*[:=]\s*[\"']?(?:bearer|basic|token)\s+(?P<v>[A-Za-z0-9._~+/=\-]{8,})"),
        0.95, "v"),
    # key=value / key: value 形式的常见凭证字段
    Rule("SECRET", re.compile(
        r"(?i)(?<![A-Za-z0-9])(?:api[_-]?key|secret[_-]?key|client[_-]?secret|access[_-]?token|"
        r"refresh[_-]?token|password|passwd|pwd|private[_-]?token|auth[_-]?token)"
        r"\s*[\"']?\s*[:=]\s*[\"']?(?P<v>[^\s\"',;，。；]{6,})"),
        0.9, "v"),
    # Explicit credential labels in Chinese and environment/token assignments.
    Rule("SECRET", re.compile(
        r"(?i)(?<![A-Za-z0-9_])(?:token|[A-Za-z][A-Za-z0-9_]*_PASS)"
        r"\s*[:=]\s*[\"']?(?P<v>[^\s\"',;，。；]{6,})"), 0.95, "v"),
    Rule("SECRET", re.compile(
        r"(?:密码|口令)(?:\s*[:=]\s*|\s+)[\"']?(?P<v>[^\s\"',;，。；]{6,})"), 0.95, "v"),
    # Known provider token shapes; values in tests are synthesized at runtime.
    Rule("SECRET", re.compile(r"(?<![A-Za-z0-9_])sk-proj-[A-Za-z0-9_-]{20,}(?![A-Za-z0-9_-])"), 0.99),
    Rule("SECRET", re.compile(r"(?<![A-Za-z0-9_])AIza[A-Za-z0-9_-]{35}(?![A-Za-z0-9_-])"), 0.99),
    # PEM 私钥块
    Rule("SECRET", re.compile(
        r"-----BEGIN (?:[A-Z ]+ )?PRIVATE KEY-----[\s\S]+?-----END (?:[A-Z ]+ )?PRIVATE KEY-----"), 0.99),
]

# ---------------------------------------------------------------------------
# 联系方式
# ---------------------------------------------------------------------------
CONTACT_RULES = [
    Rule("EMAIL", re.compile(
        r"(?<![A-Za-z0-9._%+\-])[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}(?![A-Za-z])"),
        0.9),
    # 中国大陆手机号
    Rule("CONTACT", re.compile(r"(?<!\d)1[3-9]\d{9}(?!\d)"), 0.85),
    # 国际格式电话，如 +1-202-555-0143 / +86 21 5555 1234
    Rule("CONTACT", re.compile(r"(?<![\w+])\+\d{1,3}[\- ]\d{2,4}[\- ]\d{3,4}(?:[\- ]\d{3,4})?(?!\d)"), 0.85),
    # 中国固话 021-12345678
    Rule("CONTACT", re.compile(r"(?<!\d)0\d{2,3}-\d{7,8}(?!\d)"), 0.75),
]

# ---------------------------------------------------------------------------
# 工单 / 批次 与 设备编号：同一种 "前缀-编号" 格式，按行业惯用前缀区分
# ---------------------------------------------------------------------------
DEVICE_PREFIXES = {
    "CNC", "PLC", "ROBOT", "RB", "PRESS", "LINE", "AGV", "PUMP", "MOTOR", "HMI", "FURNACE",
    "OVEN", "CMM", "LASER", "WELD", "MILL", "LATHE", "CONV", "COMP", "BOILER", "SENSOR",
    "VFD", "SERVO", "DRYER", "CHILLER", "MOLD", "IMM", "SMT", "AOI", "SPI",
}
ORDER_PREFIXES = {
    "WO", "MO", "SO", "PO", "QC", "BATCH", "LOT", "NCR", "ECN", "ECR", "RMA", "INV", "PR",
    "CR", "DN", "GRN", "TKT", "REQ", "SN", "CAPA", "DEV", "OA", "QN",
}
_CODE_RE = re.compile(rf"{_L}(?P<prefix>[A-Z]{{2,7}})-(?P<num>[A-Z]?\d{{2,8}}[A-Z]?){_R}")


class CodeRecognizer(EntityRecognizer):
    """识别 "前缀-编号"，按前缀判断是设备号还是工单/批次号。

    未知前缀：编号部分 ≥4 位数字视为单据号（ORDER_ID），否则视为设备号。
    """

    def __init__(self):
        super().__init__(supported_entities=["ORDER_ID", "DEVICE_ID"], name="IndustrialCodeRecognizer",
                         supported_language=LANG)

    def load(self) -> None:
        pass

    def analyze(self, text, entities, nlp_artifacts=None):
        out = []
        for m in _CODE_RE.finditer(text):
            prefix, num = m.group("prefix"), m.group("num")
            if prefix in DEVICE_PREFIXES:
                etype, score = "DEVICE_ID", 0.85
            elif prefix in ORDER_PREFIXES:
                etype, score = "ORDER_ID", 0.85
            else:
                digits = sum(c.isdigit() for c in num)
                etype, score = ("ORDER_ID", 0.6) if digits >= 4 else ("DEVICE_ID", 0.55)
            if entities and etype not in entities:
                continue
            out.append(RecognizerResult(etype, m.start(), m.end(), score,
                                        recognition_metadata={"recognizer_name": self.name, "source": "rule"}))
        return out


# ---------------------------------------------------------------------------
# 金额与生产指标
# ---------------------------------------------------------------------------
_NUM = r"\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?"
AMOUNT_RULES = [
    Rule("AMOUNT", re.compile(
        rf"{_L}(?:USD|CAD|EUR|GBP|RMB|CNY|JPY|HKD|AUD|SGD|US\$|\$|¥|€|£)\s?(?:{_NUM})"
        r"(?:\s?(?:k|K|M|million|thousand|bn|billion)(?![A-Za-z]))?"), 0.85),
    Rule("AMOUNT", re.compile(rf"(?<![\d.,])(?:{_NUM})\s?(?:万元|亿元|万美元|美元|欧元|元)"), 0.85),
]

_ZH_METRIC = "良率|合格率|不良率|直通率|稼动率|温度|扭矩|压力|转速|节拍|能耗|电流|电压|振动|湿度|功率|产能|速度|OEE|CPK|Cpk"
_EN_METRIC = ("yield|temperature|torque|pressure|cycle time|OEE|Cpk|speed|throughput|scrap rate|defect rate|"
              "uptime|downtime|vibration|current|voltage|humidity|power|takt time|first pass yield")
_UNIT = (r"%|℃|°C|°F|C|F|N·m|N m|Nm|kN|s|秒|ms|min|分钟|h|小时|MPa|kPa|Pa|bar|rpm|r/min|V|A|kW|W|Hz|"
         r"mm/s|mm|μm|um|kg|t|件/小时|pcs/h|ppm")
METRIC_RULES = [
    Rule("METRIC", re.compile(
        rf"(?:{_ZH_METRIC})\s*(?:为|是|达到|:|：|=)?\s*-?\d+(?:\.\d+)?\s?(?:{_UNIT})?(?![A-Za-z0-9])"), 0.8),
    Rule("METRIC", re.compile(
        rf"(?i)(?<![A-Za-z])(?:{_EN_METRIC})\s*(?:of|at|is|was|=|:)?\s*-?\d+(?:\.\d+)?\s?(?:{_UNIT})?(?![A-Za-z0-9])"),
        0.8),
]

# ---------------------------------------------------------------------------
# 商业机密概念（INTERNAL_INFO）：可选修饰语 + 机密概念词
# 这是通用行业概念表，不是租户主数据；与评测集用词存在重叠，报告中已注明。
# ---------------------------------------------------------------------------
_ZH_SECRET_MOD = r"(?:内部|未公开的?|未发布的?|保密的?|机密的?|核心|客户定制的?|供应商|采购)?"
_ZH_SECRET_CONCEPT = (
    "报价底线|底价|折扣协议|返利条款|返利协议|返点政策|工艺窗口|工艺配方|配方参数|工艺参数表|换线策略|"
    "成本结构|成本明细|毛利目标|利润率目标|定价策略|投标策略|采购底价|商业计划|未公开路线图|产品路线图|"
    "源代码|设计图纸")
_EN_SECRET_MOD = (r"(?:(?:internal|unpublished|unreleased|confidential|proprietary|customer-specific|"
                  r"supplier|undisclosed|secret)\s+)?")
_EN_SECRET_CONCEPT = (
    "price floor|floor price|rebate terms|rebate agreement|discount agreement|process window|process recipe|"
    "recipe parameters|changeover strategy|cost structure|cost breakdown|margin targets?|pricing strategy|"
    "bid strategy|product roadmap|source code|design drawings")
INTERNAL_INFO_RULES = [
    Rule("INTERNAL_INFO", re.compile(rf"{_ZH_SECRET_MOD}(?:{_ZH_SECRET_CONCEPT})"), 0.75),
    Rule("INTERNAL_INFO", re.compile(rf"(?i)(?<![A-Za-z]){_EN_SECRET_MOD}(?:{_EN_SECRET_CONCEPT})(?![A-Za-z])"),
         0.75),
]

# ---------------------------------------------------------------------------
# 组织 / 项目 / 厂区 / 人员（无主数据时的通用启发式规则；精度有限，GLiNER 应在此处补强）
# ---------------------------------------------------------------------------
_ZH_COMPANY_SUFFIX = ("精工|传动|动力|电子|装备|新材料|新材|微控|模具|机电|智造|重工|科技|机械|制造|电气|自动化|"
                      "半导体|光电|材料|汽配|集团|实业|股份|有限公司")
_EN_COMPANY_SUFFIX = ("Precision|Robotics|Electronics|Manufacturing|Components|Motors|Dynamics|Industries|"
                      "Systems|Technologies|Automation|Machinery|Metals|Materials|Works|Inc\\.?|Ltd\\.?|"
                      "Corp\\.?|GmbH|LLC|Co\\.")
_SURNAMES = ("王李张刘陈杨黄赵吴周徐孙马朱胡郭何高林罗郑梁谢宋唐许韩冯邓曹彭曾肖田董袁潘于蒋蔡余杜叶程苏魏吕丁任沈"
             "姚卢姜崔钟谭陆汪范金石廖贾夏韦付方白邹孟熊秦邱江尹薛闫段雷侯龙史陶黎贺顾毛郝龚邵万钱严覃武戴莫孔汤")
_TITLES = "工程师|经理|主管|总监|主任|厂长|组长|班长|博士|老师|师傅|总|工"
_EN_NAME = r"[A-Z][a-z]+ [A-Z][a-z]+"
_PLANT_STOP = "厂交付含包不无有该此各本个其"
_GONG_WORDS = "单厂艺程作具序位件资人业装期时种"

ORG_RULES = [
    Rule("CUSTOMER", re.compile(rf"{_ZH_CHAR}{{2}}(?:{_ZH_COMPANY_SUFFIX})"), 0.6),
    Rule("CUSTOMER", re.compile(
        rf"(?<![A-Za-z])(?:[A-Z][a-z]+ ){{1,2}}(?:{_EN_COMPANY_SUFFIX}|[A-Z][a-z]*works)(?![A-Za-z])"), 0.6),
    # "项目+两字代号"；排除 "项目进展/项目经理" 等普通词
    Rule("PROJECT", re.compile(r"项目(?!进展|延期|计划|预算|经理|管理|组|部|名称|编号)[一-鿿]{2}"), 0.6),
    Rule("PROJECT", re.compile(r"(?<![A-Za-z])Project [A-Z][a-z]+(?![A-Za-z])"), 0.65),
    # 厂区：前缀中不允许出现 "厂" 与常见虚词/动词/否定词，避免 "工厂临港一厂"、"不包含工厂" 这类误命中
    Rule("PLANT", re.compile(
        rf"(?:(?![{_ZH_STOP}{_PLANT_STOP}])[一-鿿]){{2,4}}(?:一厂|二厂|三厂|四厂|五厂|分厂|工厂|基地|车间|产业园)"), 0.6),
    Rule("PLANT", re.compile(
        r"(?<![A-Za-z])(?:Plant [A-Z][a-z]+|(?:[A-Z][a-z]+ ){1,2}(?:Hall|Factory|Cell|Plant|Workshop|Site|Campus)"
        r" [A-Z0-9]{1,3})(?![A-Za-z0-9])"), 0.6),
    # 姓 + 职务称谓；单字 "工" 后面若接 单/厂/艺/程 等，说明是 "工单/工厂/工艺" 之类普通词，不是 "周工"
    Rule("PERSON", re.compile(rf"[{_SURNAMES}](?:{_TITLES})(?![{_GONG_WORDS}])"), 0.6),
    # 英文人名：依赖上下文（角色/联系人关键词之后，或汇报类动词之前）
    Rule("PERSON", re.compile(
        rf"(?i:owner|contact|assignee|engineer|manager|by|from|cc|minutes:)(?:\s+|[\"']?\s*[:=]\s*[\"']?)"
        rf"(?P<v>{_EN_NAME})(?![A-Za-z])"),
        0.6, "v"),
    Rule("PERSON", re.compile(
        rf"(?<![A-Za-z])(?P<v>{_EN_NAME})(?= (?:presented|reported|said|approved|confirmed|requested|signed))"),
        0.6, "v"),
]


class MasterDataRecognizer(EntityRecognizer):
    """租户主数据词典识别器：把配置里的客户、项目、厂区、人员等名单精确匹配到文本中。

    这模拟企业从 CRM / MES / HR 系统同步的名单；名单按长度降序匹配，优先命中更长的名称。
    """

    def __init__(self, master_data: dict[str, list[str]]):
        self.terms: list[tuple[str, str]] = []
        for etype, values in master_data.items():
            for v in values:
                if v and v.strip():
                    self.terms.append((etype, v.strip()))
        self.terms.sort(key=lambda t: -len(t[1]))
        # 每类实体合成一个大正则，避免逐词扫描
        self.patterns: dict[str, re.Pattern[str]] = {}
        by_type: dict[str, list[str]] = {}
        for etype, v in self.terms:
            by_type.setdefault(etype, []).append(re.escape(v))
        for etype, alts in by_type.items():
            # ASCII 名称两侧加边界，避免 "Plant East" 命中 "Plant Eastern"
            self.patterns[etype] = re.compile(rf"(?<![A-Za-z0-9])(?:{'|'.join(alts)})(?![A-Za-z0-9])")
        super().__init__(supported_entities=sorted(by_type) or ["CUSTOMER"], name="MasterDataRecognizer",
                         supported_language=LANG)

    def load(self) -> None:
        pass

    def analyze(self, text, entities, nlp_artifacts=None):
        out = []
        for etype, pattern in self.patterns.items():
            if entities and etype not in entities:
                continue
            for m in pattern.finditer(text):
                out.append(RecognizerResult(etype, m.start(), m.end(), 0.99, recognition_metadata={
                    "recognizer_name": self.name, "source": "master_data"}))
        return out


def builtin_recognizers() -> list[EntityRecognizer]:
    """返回全部通用规则识别器（不含主数据与 detect-secrets）。"""
    return [
        RegexRecognizer("SecretPatternRecognizer", SECRET_RULES),
        RegexRecognizer("ContactRecognizer", CONTACT_RULES),
        CodeRecognizer(),
        RegexRecognizer("AmountRecognizer", AMOUNT_RULES),
        RegexRecognizer("MetricRecognizer", METRIC_RULES),
        RegexRecognizer("ConfidentialTermRecognizer", INTERNAL_INFO_RULES),
        RegexRecognizer("OrgHeuristicRecognizer", ORG_RULES),
    ]
