"""700 条合成评测集的离线评测脚本。

两种口径（结果分开报告，不能混为一谈）：
- A_master_data：把评测集中出现的客户/项目/厂区/人员名单作为“租户主数据”注入。
  模拟企业已从 CRM/MES/HR 同步名单的上线状态。注意：名单来自评测集本身，
  因此 A 口径下这四类实体的召回近似“上限”，不代表对未知名称的识别能力。
- B_no_dictionary：不注入任何名单，只靠通用规则 + detect-secrets，衡量泛化能力。

评测集字段 → 请求的映射（属于“调用方上下文”，不是识别能力）：
- channel → 已登记目的地 registered-<channel>（trust=external_untrusted，见 README 第“策略冲突”一节）
- task_invariants 中含 "amount retained" → constraints=["preserve:AMOUNT"]（调用方声明经批准的计算用途）
- 每行使用独立 task_id（= 样本 id），Token 编号在行内从 001 开始

用法：
  uv run python -m bench.run_benchmark --out bench/results
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import statistics
import sys
import tempfile
import time
from collections import Counter, defaultdict
from pathlib import Path

from agentnexus_dlp.models import ALLOW, ANONYMIZE, BLOCK, REVIEW, EvaluateRequest
from agentnexus_dlp.service import EgressService, ServiceConfig

DECISIONS = [ALLOW, ANONYMIZE, REVIEW, BLOCK]
MASTER_TYPES = ("CUSTOMER", "PROJECT", "PLANT", "PERSON")
TENANT, ACTOR = "bench-tenant", "bench-actor"


def load_rows(path: Path) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def master_data_from_rows(rows: list[dict]) -> dict[str, list[str]]:
    md: dict[str, set[str]] = defaultdict(set)
    for r in rows:
        for e in r["entities"]:
            if e["type"] in MASTER_TYPES:
                md[e["type"]].add(e["text"])
    return {k: sorted(v) for k, v in md.items()}


def constraints_for(row: dict) -> list[str]:
    if "constraints" in row:  # 留出集直接给出约束
        return list(row["constraints"])
    inv = " ".join(row.get("task_invariants") or [])
    return ["preserve:AMOUNT"] if "amount retained" in inv else []


def _mask_secrets(text: str, row: dict) -> str:
    """报告中不写出凭证值（即便是合成的）：漏放样本的输出里会含原值，改为 <SECRET:长度>。"""
    for e in row["entities"]:
        if e["type"] == "SECRET":
            text = text.replace(e["text"], f"<SECRET:{len(e['text'])}>")
    return text


def prf(tp: int, fp: int, fn: int) -> dict:
    p = tp / (tp + fp) if tp + fp else 0.0
    r = tp / (tp + fn) if tp + fn else 0.0
    f = 2 * p * r / (p + r) if p + r else 0.0
    return {"precision": round(p, 4), "recall": round(r, 4), "f1": round(f, 4), "tp": tp, "fp": fp, "fn": fn,
            "support": tp + fn}


def pct(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    k = max(0, min(len(s) - 1, int(round(q / 100 * len(s) + 0.5)) - 1))
    return round(s[k], 3)


def run_benchmark(path: str | Path | None, mode: str, workdir: str | Path, rows: list[dict] | None = None) -> dict:
    rows = rows if rows is not None else load_rows(Path(path))
    channels = sorted({r["channel"] for r in rows})
    destinations = {f"registered-{c}": {"trust": "external_untrusted"} for c in channels}
    master = master_data_from_rows(rows) if mode == "A_master_data" else None
    key = base64.urlsafe_b64encode(os.urandom(32))
    svc = EgressService(ServiceConfig(destinations=destinations, master_data=master,
                                      vault_path=Path(workdir) / f"vault-{mode}.sqlite3", vault_key=key))
    try:
        return _run(svc, rows, mode)
    finally:
        svc.close()


def _run(svc: EgressService, rows: list[dict], mode: str) -> dict:
    # 预热一次（模型/正则编译），不计入延迟
    svc.evaluate(EvaluateRequest(TENANT, ACTOR, "warmup", "email", "registered-email", "warmup", text="预热"))

    ent_counts = defaultdict(lambda: [0, 0, 0])  # type -> [tp, fp, fn]
    lang_counts = defaultdict(lambda: [0, 0, 0])
    boundary_near_miss = 0
    confusion = {e: {p: 0 for p in DECISIONS} for e in DECISIONS}
    by_channel = defaultdict(lambda: [0, 0])  # channel -> [decision correct, total]
    exact = Counter()
    restore_stats = Counter()
    invariants = Counter()
    safety = Counter()
    latencies: list[float] = []
    failures: list[dict] = []
    errors = 0

    for row in rows:
        src = row["source_text"]
        req = EvaluateRequest(TENANT, ACTOR, row["id"], row["channel"], f"registered-{row['channel']}",
                              "dataset_regression", text=src, constraints=constraints_for(row),
                              language=row["language"])
        res = svc.evaluate(req)
        latencies.append(res.elapsed_ms)
        if "internal_error_fail_closed" in res.reason_codes:
            errors += 1

        # ---- 实体（严格匹配 type,start,end）
        gold = {(e["type"], e["start"], e["end"]) for e in row["entities"]}
        pred = {(e.type, e.start, e.end) for e in res.entities}
        for t in {g[0] for g in gold} | {p[0] for p in pred}:
            g_t, p_t = {x for x in gold if x[0] == t}, {x for x in pred if x[0] == t}
            tp = len(g_t & p_t)
            ent_counts[t][0] += tp
            ent_counts[t][1] += len(p_t - g_t)
            ent_counts[t][2] += len(g_t - p_t)
            lang_counts[row["language"]][0] += tp
            lang_counts[row["language"]][1] += len(p_t - g_t)
            lang_counts[row["language"]][2] += len(g_t - p_t)
        # 类型相同、区间重叠但边界不一致的“擦边”数量
        for g in gold - pred:
            if any(p[0] == g[0] and p[1] < g[2] and g[1] < p[2] for p in pred - gold):
                boundary_near_miss += 1

        # ---- 决策
        exp, got = row["expected_action"], res.decision
        confusion[exp][got] += 1
        by_channel[row["channel"]][1] += 1
        by_channel[row["channel"]][0] += int(exp == got)
        if exp in (BLOCK, REVIEW) and got in (ALLOW, ANONYMIZE):
            safety["dangerous_release"] += 1
        if exp in (ALLOW, ANONYMIZE) and got in (REVIEW, BLOCK):
            safety["over_blocking"] += 1

        # ---- 凭证
        out_text = res.sanitized_text or ""
        resp_json = json.dumps(res.to_dict(), ensure_ascii=False)
        for e in row["entities"]:
            if e["type"] == "SECRET":
                safety["secret_total"] += 1
                if got != BLOCK or e["text"] in out_text:
                    safety["secret_missed"] += 1
                if e["text"] in resp_json:
                    safety["secret_value_in_output"] += 1

        # ---- 脱敏文本完全一致
        exact["total"] += 1
        if out_text == row["expected_sanitized_text"]:
            exact["match"] += 1
        elif len(failures) < 400:
            failures.append({"id": row["id"], "expected_action": exp, "got_action": got,
                             "expected": _mask_secrets(row["expected_sanitized_text"], row),
                             "got": _mask_secrets(out_text, row)})

        # ---- 任务保真不变量（以金标实体为准）
        for e in row["entities"]:
            if e["type"] == "EMAIL" and "expected_restoration_map" not in row:
                if e["text"] in row["expected_sanitized_text"]:
                    invariants["routing_total"] += 1
                    invariants["routing_kept"] += int(e["text"] in out_text)
                continue
            if e["type"] == "METRIC":
                invariants["metric_total"] += 1
                invariants["metric_kept"] += int(e["text"] in out_text)
            if e["type"] == "DEVICE_ID":
                invariants["device_total"] += 1
                invariants["device_kept"] += int(e["text"] in out_text)
            if e["type"] == "EMAIL" and e["text"] in row["expected_sanitized_text"]:
                invariants["routing_total"] += 1
                invariants["routing_kept"] += int(e["text"] in out_text)

        # ---- Vault：恢复映射与往返
        if row.get("expected_restoration_map") and got in (ANONYMIZE, REVIEW):
            restore_stats["map_total"] += 1
            got_map = {}
            for tok in res.tokens:
                back = svc.restore(TENANT, ACTOR, row["id"], tok, "local_ui")
                got_map[tok] = back.text
            restore_stats["map_match"] += int(got_map == row["expected_restoration_map"])
        if got == ANONYMIZE and res.tokens:
            restore_stats["roundtrip_total"] += 1
            back = svc.restore(TENANT, ACTOR, row["id"], out_text, "local_ui")
            restore_stats["roundtrip_ok"] += int(back.text == src)
            other_task = svc.restore(TENANT, ACTOR, row["id"] + "-other", out_text, "local_ui")
            other_tenant = svc.restore("other-tenant", ACTOR, row["id"], out_text, "local_ui")
            restore_stats["cross_scope_attempts"] += 1
            restore_stats["cross_task_restored"] += other_task.restored
            restore_stats["cross_tenant_restored"] += other_tenant.restored

    # ---- 补充用例：未登记目的地必须 BLOCK（评测集本身全部是已登记外部接收方）
    supp = Counter()
    for row in rows[::35]:
        r = svc.evaluate(EvaluateRequest(TENANT, ACTOR, row["id"] + "-unk", row["channel"], "unregistered-paste-site",
                                         "x", text=row["source_text"]))
        supp["unknown_destination_total"] += 1
        supp["unknown_destination_blocked"] += int(r.decision == BLOCK and r.sanitized_text is None)

    per_type = {t: prf(*v) for t, v in sorted(ent_counts.items())}
    tp, fp, fn = (sum(v[i] for v in ent_counts.values()) for i in range(3))
    macro_f1 = statistics.mean(m["f1"] for m in per_type.values()) if per_type else 0.0
    correct = sum(confusion[d][d] for d in DECISIONS)
    return {
        "mode": mode,
        "rows": len(rows),
        "errors": errors,
        "entities": {"micro": prf(tp, fp, fn), "macro_f1": round(macro_f1, 4), "per_type": per_type,
                     "per_language": {k: prf(*v) for k, v in sorted(lang_counts.items())},
                     "boundary_near_miss": boundary_near_miss},
        "decisions": {"accuracy": round(correct / len(rows), 4), "confusion": confusion,
                      "per_channel_accuracy": {c: round(a / n, 4) for c, (a, n) in sorted(by_channel.items())}},
        "safety": {"dangerous_release": safety["dangerous_release"], "over_blocking": safety["over_blocking"],
                   "secret_total": safety["secret_total"], "secret_missed": safety["secret_missed"],
                   "secret_value_in_output": safety["secret_value_in_output"]},
        "sanitized_exact_match": {"match": exact["match"], "total": exact["total"],
                                  "rate": round(exact["match"] / exact["total"], 4)},
        "invariants": dict(invariants),
        "vault": {"restoration_map_match": restore_stats["map_match"],
                  "restoration_map_total": restore_stats["map_total"],
                  "roundtrip_ok": restore_stats["roundtrip_ok"], "roundtrip_total": restore_stats["roundtrip_total"],
                  "cross_scope_attempts": restore_stats["cross_scope_attempts"],
                  "cross_task_restored": restore_stats["cross_task_restored"],
                  "cross_tenant_restored": restore_stats["cross_tenant_restored"]},
        "supplementary": dict(supp),
        "latency_ms": {"p50": pct(latencies, 50), "p95": pct(latencies, 95), "p99": pct(latencies, 99),
                       "max": round(max(latencies), 3)},
        "failures": failures,
    }


def to_markdown(reports: list[dict], meta: dict) -> str:
    lines = ["# 700 条评测结果", "", f"- 数据集：`{meta['dataset']}`（sha256 `{meta['sha256'][:16]}…`）",
             f"- 运行时间：{meta['time']}；Python {meta['python']}；GLiNER：未启用", ""]
    for r in reports:
        e, d, s = r["entities"], r["decisions"], r["safety"]
        lines += [f"## 口径 {r['mode']}", "",
                  "| 指标 | 结果 |", "|---|---|",
                  f"| 样本 / 异常 | {r['rows']} / {r['errors']} |",
                  f"| 实体 micro P / R / F1 | {e['micro']['precision']} / {e['micro']['recall']} / {e['micro']['f1']} |",
                  f"| 实体 macro F1 | {e['macro_f1']} |",
                  f"| 决策准确率 | {d['accuracy']} |",
                  f"| 危险放行（应 BLOCK/REVIEW 却放行） | {s['dangerous_release']} |",
                  f"| 过度拦截（应放行却 REVIEW/BLOCK） | {s['over_blocking']} |",
                  f"| 凭证漏放 / 凭证值出现在响应中 | {s['secret_missed']} / {s['secret_value_in_output']}"
                  f"（共 {s['secret_total']}） |",
                  f"| 脱敏文本完全一致 | {r['sanitized_exact_match']['match']}/{r['sanitized_exact_match']['total']}"
                  f" = {r['sanitized_exact_match']['rate']} |",
                  f"| 恢复映射与期望一致 | {r['vault']['restoration_map_match']}/{r['vault']['restoration_map_total']} |",
                  f"| 往返恢复 = 原文 | {r['vault']['roundtrip_ok']}/{r['vault']['roundtrip_total']} |",
                  f"| 跨任务 / 跨租户恢复成功次数 | {r['vault']['cross_task_restored']} / "
                  f"{r['vault']['cross_tenant_restored']}（尝试 {r['vault']['cross_scope_attempts']}） |",
                  f"| 指标保留 / 设备号保留 / 收件人保留 | {r['invariants'].get('metric_kept', 0)}/"
                  f"{r['invariants'].get('metric_total', 0)}，{r['invariants'].get('device_kept', 0)}/"
                  f"{r['invariants'].get('device_total', 0)}，{r['invariants'].get('routing_kept', 0)}/"
                  f"{r['invariants'].get('routing_total', 0)} |",
                  f"| 未登记目的地被 BLOCK | {r['supplementary']['unknown_destination_blocked']}/"
                  f"{r['supplementary']['unknown_destination_total']} |",
                  f"| 单条评估延迟 P50 / P95 / P99（ms） | {r['latency_ms']['p50']} / {r['latency_ms']['p95']} / "
                  f"{r['latency_ms']['p99']} |", "",
                  "决策混淆矩阵（行 = 期望，列 = 实际）：", "",
                  "| 期望 \\ 实际 | " + " | ".join(DECISIONS) + " |", "|---|" + "---|" * len(DECISIONS)]
        for exp in DECISIONS:
            lines.append(f"| {exp} | " + " | ".join(str(d["confusion"][exp][g]) for g in DECISIONS) + " |")
        lines += ["", "分实体类型（严格匹配 type+start+end）：", "",
                  "| 类型 | 支持数 | P | R | F1 |", "|---|---|---|---|---|"]
        for t, m in e["per_type"].items():
            lines.append(f"| {t} | {m['support']} | {m['precision']} | {m['recall']} | {m['f1']} |")
        lines += ["", "分语言：" + "；".join(f"{k} F1={v['f1']}" for k, v in e["per_language"].items())
                  + f"；边界偏移（类型对、区间重叠但不完全一致）{e['boundary_near_miss']} 处", ""]
    return "\n".join(lines)


def dump_json(obj) -> str:
    """JSON 输出时把不可见格式字符写成 \\uXXXX 转义：内容等价，但不会在仓库里留下不可见字符
    （CI 的提示注入扫描会阻断含不可见字符的文件）。"""
    import unicodedata
    text = json.dumps(obj, ensure_ascii=False, indent=2)
    return "".join(f"\\u{ord(c):04x}" if unicodedata.category(c) == "Cf" else c for c in text)


def main(argv: list[str] | None = None) -> int:
    import hashlib
    import platform

    ap = argparse.ArgumentParser(description="运行 700 条外发脱敏评测")
    ap.add_argument("--data", default=str(Path(__file__).parent / "data" / "agentnexus_dlp_700.jsonl"))
    ap.add_argument("--out", default=str(Path(__file__).parent / "results"))
    args = ap.parse_args(argv)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    reports = []
    with tempfile.TemporaryDirectory() as tmp:  # Vault 文件放临时目录，评测后删除
        for mode in ("A_master_data", "B_no_dictionary"):
            t0 = time.perf_counter()
            rep = run_benchmark(args.data, mode, tmp)
            rep["wall_seconds"] = round(time.perf_counter() - t0, 2)
            reports.append(rep)
            (out / f"report_{mode}.json").write_text(dump_json(rep), "utf-8")
        from bench.holdout import holdout_rows
        ho = holdout_rows()
        for mode in ("A_master_data", "B_no_dictionary"):
            rep = run_benchmark(None, mode, tmp, rows=ho)
            rep["mode"] = f"HOLDOUT_{mode}"
            reports.append(rep)
            (out / f"report_HOLDOUT_{mode}.json").write_text(dump_json(rep), "utf-8")
    meta = {"dataset": Path(args.data).name, "sha256": hashlib.sha256(Path(args.data).read_bytes()).hexdigest(),
            "time": time.strftime("%Y-%m-%d %H:%M:%S"), "python": platform.python_version()}
    (out / "SUMMARY.md").write_text(to_markdown(reports, meta), "utf-8")
    print(to_markdown(reports, meta))
    return 0


if __name__ == "__main__":
    sys.exit(main())
