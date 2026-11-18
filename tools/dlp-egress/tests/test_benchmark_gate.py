"""700 条评测集的安全门槛（硬性验收条件）。

只断言安全底线：凭证零漏放、应 BLOCK/REVIEW 的样本零误放、跨任务恢复零成功。
准确率等指标由 bench/run_benchmark.py 输出报告，不在这里设阈值，避免为凑分调整断言。
"""
from pathlib import Path

import pytest

from bench.run_benchmark import run_benchmark

DATA = Path(__file__).resolve().parent.parent / "bench" / "data" / "agentnexus_dlp_700.jsonl"


@pytest.fixture(scope="module", params=["A_master_data", "B_no_dictionary"])
def report(request, tmp_path_factory):
    return run_benchmark(DATA, mode=request.param, workdir=tmp_path_factory.mktemp(request.param))


def test_all_700_rows_evaluated(report):
    assert report["rows"] == 700
    assert report["errors"] == 0


def test_zero_secret_leak(report):
    assert report["safety"]["secret_missed"] == 0
    assert report["safety"]["secret_value_in_output"] == 0


def test_zero_dangerous_release(report):
    # 期望 BLOCK/REVIEW 却被判为 ALLOW/ANONYMIZE（即会被直接外发）的条数
    assert report["safety"]["dangerous_release"] == 0


def test_zero_cross_scope_restore(report):
    assert report["vault"]["cross_task_restored"] == 0
    assert report["vault"]["cross_tenant_restored"] == 0


def test_supplementary_unknown_destination_blocked(report):
    supp = report["supplementary"]
    assert supp["unknown_destination_blocked"] == supp["unknown_destination_total"]


@pytest.fixture(scope="module", params=["A_master_data", "B_no_dictionary"])
def holdout_report(request, tmp_path_factory):
    from bench.holdout import holdout_rows
    return run_benchmark(None, request.param, tmp_path_factory.mktemp("ho-" + request.param), rows=holdout_rows())


def test_holdout_safety_floor(holdout_report):
    # 独立留出集上的安全底线（识别准确率只报告、不设门槛）
    assert holdout_report["errors"] == 0
    assert holdout_report["safety"]["secret_missed"] == 0
    assert holdout_report["safety"]["secret_value_in_output"] == 0
    assert holdout_report["safety"]["dangerous_release"] == 0
