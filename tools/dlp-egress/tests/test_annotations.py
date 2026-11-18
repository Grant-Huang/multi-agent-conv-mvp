"""标注格式解析、期望结果推导与转换脚本的测试（对应 docs/ANNOTATION_GUIDE.md）。"""
import csv
import json

import pytest

from bench.annotation import AnnotationError, derive_expected, parse_markup
from bench.convert_annotations import convert


def test_parse_markup_offsets_and_escaped_braces():
    src, ents = parse_markup("参数{{x}}：{CUSTOMER:松岩液压}的{ORDER_ID:PO-771203}")
    assert src == "参数{x}：松岩液压的PO-771203"
    assert [(e["type"], src[e["start"]:e["end"]]) for e in ents] == [("CUSTOMER", "松岩液压"), ("ORDER_ID", "PO-771203")]


@pytest.mark.parametrize("bad, msg", [
    ("{COMPANY:松岩液压}", "未知实体类型"),
    ("{CUSTOMER:松岩{PLANT:一厂}}", "不允许嵌套"),
    ("{CUSTOMER:松岩液压", "未闭合"),
    ("多余的右括号}", "未配对"),
    ("{CUSTOMER:}", "实体文本为空"),
])
def test_parse_markup_rejects_malformed(bad, msg):
    with pytest.raises(AnnotationError, match=msg):
        parse_markup(bad)


def test_derive_expected_external():
    src, ents = parse_markup("把{CUSTOMER:松岩液压}的{METRIC:良率97.0%}发出，含{INTERNAL_INFO:内部报价底线}")
    action, text = derive_expected(src, ents, "external_untrusted", [])
    assert action == "REVIEW"
    assert text == "把[CUSTOMER_001]的良率97.0%发出，含[REDACTED_CONFIDENTIAL]"


def test_derive_expected_internal_and_unknown():
    src, ents = parse_markup("把{CUSTOMER:松岩液压}的{INTERNAL_INFO:内部报价底线}发到内部 wiki")
    assert derive_expected(src, ents, "internal", []) == ("ALLOW", src)
    assert derive_expected(src, ents, "unknown", []) == ("BLOCK", "")


def test_derive_expected_amount_constraint_and_routing():
    src, ents = parse_markup('send_email(to="{EMAIL:qa@p.com}", body="{AMOUNT:56万元} 联系 {EMAIL:ops@p.com}")')
    action, text = derive_expected(src, ents, "external_untrusted", ["preserve:AMOUNT"])
    assert action == "ANONYMIZE"
    assert text == 'send_email(to="qa@p.com", body="56万元 联系 [EMAIL_001]")'


def _write_csv(path, rows):
    fields = ["id", "language", "channel", "scenario", "destination_trust", "purpose", "constraints",
              "annotated_text", "expected_action", "difficulty", "annotator", "reviewer", "notes"]
    with open(path, "w", encoding="utf-8-sig", newline="") as f:  # 带 BOM：模拟 Excel 导出的文件
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        for r in rows:
            w.writerow({k: r.get(k, "") for k in fields})


GOOD = {"id": "T-001", "language": "zh", "channel": "email", "scenario": "normal",
        "destination_trust": "external_untrusted", "purpose": "vendor_diagnostics", "constraints": "",
        "annotated_text": "请把{CUSTOMER:松岩液压}的{ORDER_ID:PO-771203}发给物流商", "expected_action": "ANONYMIZE",
        "difficulty": "normal", "annotator": "A", "reviewer": "B"}


def test_convert_csv_to_benchmark_jsonl(tmp_path):
    src = tmp_path / "a.csv"
    _write_csv(src, [GOOD])
    out = tmp_path / "out.jsonl"
    report = convert(src, out)
    assert report["errors"] == [] and report["disagreements"] == []
    row = json.loads(out.read_text(encoding="utf-8").strip())
    assert row["source_text"] == "请把松岩液压的PO-771203发给物流商"
    assert row["expected_sanitized_text"] == "请把[CUSTOMER_001]的[ORDER_ID_001]发给物流商"
    assert row["destination_trust"] == "external_untrusted"


def test_convert_reports_errors_duplicates_and_disagreements(tmp_path):
    bad_type = dict(GOOD, id="T-002", annotated_text="{COMPANY:x}")
    dup = dict(GOOD)
    disagree = dict(GOOD, id="T-003", expected_action="REVIEW")
    bad_lang = dict(GOOD, id="T-004", language="fr")
    src = tmp_path / "a.csv"
    _write_csv(src, [GOOD, bad_type, dup, disagree, bad_lang])
    report = convert(src, tmp_path / "out.jsonl")
    msgs = " ".join(e["message"] for e in report["errors"])
    assert "未知实体类型" in msgs and "重复" in msgs and "language" in msgs
    assert [d["id"] for d in report["disagreements"]] == ["T-003"]
    assert report["written"] == 0  # 有错误时不输出半成品


def test_convert_jsonl_input(tmp_path):
    src = tmp_path / "a.jsonl"
    src.write_text(json.dumps(GOOD, ensure_ascii=False) + "\n", encoding="utf-8")
    report = convert(src, tmp_path / "out.jsonl")
    assert report["written"] == 1


def test_template_file_is_valid():
    """仓库内的标注模板本身必须能通过校验（模板是给标注员照抄的示例）。"""
    from pathlib import Path
    tpl = Path(__file__).resolve().parent.parent / "bench" / "templates" / "annotation_template.csv"
    report = convert(tpl, None)
    assert report["errors"] == [] and report["disagreements"] == []
    assert report["valid"] >= 8
