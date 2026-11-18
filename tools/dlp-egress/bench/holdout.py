"""独立留出集（hold-out）：人工编写，词汇与句式均不来自 700 条评测集。

编写规则：本文件在运行之前定稿；运行结果只做报告，**不据此再调整识别规则**，
否则留出集也会被“调参污染”。如需新增覆盖，请新建下一版留出集。

标注写法：{TYPE:原文}，例如 "请把{CUSTOMER:松岩液压}的资料发出"。
期望决策与期望脱敏文本由“金标实体 + 默认策略（external_untrusted）”推导：
SECRET→BLOCK / INTERNAL_INFO→REVIEW / 其他可 Token 化类型→ANONYMIZE / METRIC、DEVICE_ID 保留。
route=True 表示该文本中的邮箱处于收件人路由参数中，应原样保留。
"""
from __future__ import annotations

import re

from bench import fake_secrets as fs  # 凭证样式字符串运行时拼接，避免触发仓库的密钥扫描

_MARK = re.compile(r"\{([A-Z_]+):([^{}]+)\}")
_KEEP = {"METRIC", "DEVICE_ID"}

# (id, 场景标签, 带标注文本, constraints)
CASES: list[tuple[str, str, str, list[str]]] = [
    # ---- 常规外发：新的客户名、厂区、人员、单据格式
    ("HO-001", "normal", "请把{CUSTOMER:松岩液压}的{ORDER_ID:PO-771203}到货异常同步给外部物流商。", []),
    ("HO-002", "normal", "{PLANT:滨江四厂}的{DEVICE_ID:AGV-11}本周停机两次，{METRIC:稼动率81.5%}，请供应商远程诊断。", []),
    ("HO-003", "normal", "Please ask the vendor to check {DEVICE_ID:SERVO-04} at {PLANT:Plant West}; "
                         "{METRIC:vibration 7.2 mm/s} since Monday.", []),
    ("HO-004", "normal", "{PERSON:郑厂长}要求把{ORDER_ID:NCR-20931}的处理结论发给{CUSTOMER:恒屹汽配}。", []),
    ("HO-005", "normal", "Escalate {ORDER_ID:RMA-55012} for {CUSTOMER:Granite Peak Machinery}, "
                         "owner {PERSON:Dana Brooks}.",
     []),
    ("HO-006", "normal", "外部审核员需要{PROJECT:项目长庚}的首件检验结果，{METRIC:合格率99.1%}。", []),
    ("HO-007", "normal", "Share the {PROJECT:Project Osprey} test summary with the lab; "
                         "{METRIC:takt time 55 s} is within target.", []),
    ("HO-008", "normal", "请联系{PERSON:韩主任}，电话{CONTACT:021-66881234}，确认{ORDER_ID:LOT-7Q}的批次放行。", []),
    ("HO-009", "normal", "Vendor contact: {EMAIL:li.ming@vendor-demo.io}, mobile {CONTACT:+44 20 7946 0958}.", []),
    ("HO-010", "normal", "{CUSTOMER:Northwind Automation} asked about {ORDER_ID:SO-90817} shipment; "
                         "{METRIC:yield 96.4%} last week.", []),
    # ---- 金额：默认 Token 化；带 preserve:AMOUNT 约束时保留
    ("HO-011", "amount", "向外部财务顾问说明{CUSTOMER:松岩液压}的应收款{AMOUNT:EUR 64,800}。", []),
    ("HO-012", "amount", "Quote for {CUSTOMER:Granite Peak Machinery}: {AMOUNT:$ 125,000} including tooling.",
     ["preserve:AMOUNT"]),
    ("HO-013", "amount", "合同金额{AMOUNT:96.5万元}，请第三方审计核对。", ["preserve:AMOUNT"]),
    # ---- 机密信息 → REVIEW
    ("HO-014", "confidential", "这份附件含有{INTERNAL_INFO:保密的成本结构}，客户是{CUSTOMER:恒屹汽配}。", []),
    ("HO-015", "confidential", "Do not forward our {INTERNAL_INFO:proprietary pricing strategy} "
                               "to the distributor.", []),
    ("HO-016", "confidential", "供应商想要{INTERNAL_INFO:核心工艺配方}，{PLANT:滨江四厂}暂不同意。", []),
    # ---- 凭证 → BLOCK（多种格式）
    ("HO-017", "secret", f"调试日志：{fs.PASSWORD_KEY}={{SECRET:Zx9!qLm2pR}}，准备发给外包团队。", []),
    ("HO-018", "secret", f"{fs.AWS_KEY_ID_KEY} = {{SECRET:{fs.AWS_SYNTH}}} 已写入配置并上传。", []),
    ("HO-019", "secret", "Header Authorization: Bearer "
                         f"{{SECRET:{fs.JWT_SYNTH}}} "
                         "for the partner webhook.", []),
    ("HO-020", "secret", f"token: {{SECRET:{fs.GITHUB_B}}}", []),
    ("HO-021", "secret", f"{fs.API_KEY_KEY}: \"{{SECRET:SYNTH-apikey-7Hq2LyjWDarjtT1zdp7dc}}\" "
                         "pasted into the vendor form",
     []),
    # ---- 工具调用 / 路由字段
    ("HO-022", "toolcall", 'send_email(to="qa@partner-demo.com", body="{CUSTOMER:松岩液压} {METRIC:良率97.0%}")', []),
    ("HO-023", "toolcall", 'slack.post(channel="#vendor", text="{ORDER_ID:WO-118820} delayed, ask {PERSON:Chris Wong} '
                           'reported it")', []),
    ("HO-024", "toolcall", 'calendar.invite(attendees=["ops@partner-demo.com"], title="{PROJECT:Project Osprey} sync")',
     []),
    # ---- 对抗：全角、零宽字符、同一实体多次出现、中英混排
    ("HO-025", "adversarial", "全角编号{ORDER_ID:ＷＯ－７７３０１２}请发给外部仓库。", []),
    ("HO-026", "adversarial", "客户{CUSTOMER:松\u200b岩液压}的交付计划。", []),
    ("HO-027", "adversarial", "{CUSTOMER:松岩液压}与{CUSTOMER:恒屹汽配}对比：{CUSTOMER:松岩液压}交付更快。", []),
    ("HO-028", "adversarial", "Ping {PERSON:Dana Brooks} 关于 {ORDER_ID:PO-771203} 的进度。", []),
    # ---- 困难：无职务称谓的中文人名、无行业后缀的公司名（规则预期会漏，用于暴露能力边界）
    ("HO-029", "hard", "请把报告发给{PERSON:王晓峰}，他负责{CUSTOMER:蓝鲸}项目对接。", []),
    ("HO-030", "hard", "{CUSTOMER:Acme} wants the {ORDER_ID:PO-771203} data by Friday.", []),
    ("HO-031", "hard", "Message from {PERSON:Priya Natarajan}: the batch is ready.", []),
    # ---- 干净负例：含易误报词
    ("HO-032", "clean", "项目管理培训资料：如何拆分工单与排产，适用于任何工厂。", []),
    ("HO-033", "clean", "The OEE formula is availability times performance times quality.", []),
    ("HO-034", "clean", "请参考公开标准 ISO 9001:2015 的第 8.5 条。", []),
    ("HO-035", "clean", "Generic checklist: lubricate, tighten, clean, and record results.", []),
    ("HO-036", "clean", "车间安全守则：进入现场须佩戴安全帽。", []),
]


def build_case(cid: str, tag: str, marked: str, constraints: list[str]) -> dict:
    """把带标注的文本展开成与 700 条评测集同构的样本。"""
    source, entities, pos = [], [], 0
    for m in _MARK.finditer(marked):
        source.append(marked[pos:m.start()])
        start = sum(len(s) for s in source)
        etype, text = m.group(1), m.group(2)
        source.append(text)
        entities.append({"type": etype, "start": start, "end": start + len(text), "text": text})
        pos = m.end()
    source.append(marked[pos:])
    src = "".join(source)

    routing = _routing_ranges(src)
    keep_amount = "preserve:AMOUNT" in constraints
    counters: dict[str, int] = {}
    canon: dict[tuple[str, str], str] = {}
    decision_rank = 0  # 0 ALLOW 1 ANONYMIZE 2 REVIEW 3 BLOCK
    pieces, pos = [], 0
    for e in entities:
        pieces.append(src[pos:e["start"]])
        t = e["type"]
        in_route = any(a <= e["start"] and e["end"] <= b for a, b in routing)
        if t == "SECRET":
            rep, decision_rank = "[BLOCKED_SECRET]", max(decision_rank, 3)
        elif t == "INTERNAL_INFO":
            rep, decision_rank = "[REDACTED_CONFIDENTIAL]", max(decision_rank, 2)
        elif t in _KEEP or (t == "AMOUNT" and keep_amount) or (t == "EMAIL" and in_route):
            rep = e["text"]
        else:
            key = (t, _canonical(e["text"]))
            if key not in canon:
                counters[t] = counters.get(t, 0) + 1
                canon[key] = f"[{t}_{counters[t]:03d}]"
            rep, decision_rank = canon[key], max(decision_rank, 1)
        pieces.append(rep)
        pos = e["end"]
    pieces.append(src[pos:])
    action = ["ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"][decision_rank]
    return {"id": cid, "scenario": tag, "language": "zh" if re.search(r"[一-鿿]", src) else "en",
            "channel": "external_api", "source_text": src, "entities": entities, "expected_action": action,
            "expected_sanitized_text": "".join(pieces), "constraints": constraints}


def _canonical(v: str) -> str:
    import unicodedata
    v = "".join(c for c in v if unicodedata.category(c) != "Cf")
    return " ".join(unicodedata.normalize("NFKC", v).split()).casefold()


def _routing_ranges(text: str) -> list[tuple[int, int]]:
    pat = re.compile(r"(?i)(?<![A-Za-z_])(?:to|cc|bcc|guests|recipients|attendees)\s*[=:]\s*(\[[^\]]*\]|\"[^\"]*\")")
    return [m.span(1) for m in pat.finditer(text)]


def holdout_rows() -> list[dict]:
    return [build_case(*c) for c in CASES]
