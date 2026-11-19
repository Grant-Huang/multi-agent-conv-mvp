"""评测口径测试：服务在识别之前就 BLOCK 的行（如目的地未登记）不计入实体指标。"""
from bench.annotation import derive_expected, parse_markup
from bench.run_benchmark import run_benchmark


def _row(rid, marked, trust):
    src, ents = parse_markup(marked)
    action, sanitized = derive_expected(src, ents, trust, [])
    return {"id": rid, "language": "zh", "channel": "email", "destination_trust": trust, "source_text": src,
            "entities": ents, "expected_action": action, "expected_sanitized_text": sanitized, "constraints": []}


def test_rows_blocked_before_detection_are_not_scored(tmp_path):
    rows = [_row("U-1", "把{ORDER_ID:PO-771203}发到陌生网站", "unknown"),
            _row("K-1", "请处理{ORDER_ID:PO-771204}", "external_untrusted")]
    rep = run_benchmark(None, "B_no_dictionary", tmp_path, rows=rows)
    assert rep["entities"]["unscored_rows"] == 1
    assert rep["entities"]["micro"]["support"] == 1      # 只统计执行了识别的那一行
    assert rep["entities"]["micro"]["recall"] == 1.0
    assert rep["decisions"]["accuracy"] == 1.0            # 决策仍然全部计入
