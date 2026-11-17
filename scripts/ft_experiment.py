#!/usr/bin/env python3
"""FT 前台提速实验：按顺序跑 E0~E5 六组配置，汇总成一份对比表。

用法（在仓库根目录、本机模型服务已启动时）：
    python3 scripts/ft_experiment.py                      # 用 config.toml 和 evals/routing.jsonl
    python3 scripts/ft_experiment.py --config my.toml --variants E0 E1 E4

做法：每组调用一次
    cargo run --release --bin an-eval -- <config> <dataset> --variant Ex --label FT-Ex
an-eval 只在内存里覆盖配置（见 crates/server/src/bin/eval.rs 的 --variant），不会修改配置文件。

输出：
    evals/results/<时间>-FT-Ex-<审查模式>.summary.json / .jsonl   每组的原始结果（an-eval 生成）
    evals/results/ft-report-<时间>.md / .json                     六组对比（本脚本生成）
把 ft-report 的 .md 和 .json 发回即可分析；需要看错例时再发对应的 .jsonl。

只用 Python 标准库。
"""

import argparse
import glob
import json
import os
import subprocess
import sys
import time

# 每组配置的含义（与 an-eval 的 --variant 一致）
VARIANTS = {
    "E0": "开思考（基线）",
    "E1": "关思考",
    "E2": "关思考 + A 先写理由",
    "E3": "关思考 + A + B 规则判风险",
    "E4": "关思考 + A + B + D 可疑时重判（完整方案）",
    "E5": "开思考 + B（看 B 对现有默认有无副作用）",
}

# 验收标准（用户 2026-11-17 确认：准确率优先，P95 放宽到 6 秒）
TARGET_PASS = 0.853
TARGET_P95_MS = 6000


def run_variant(variant, config, dataset):
    """跑一组配置，返回它生成的 summary.json 内容；失败返回 None。"""
    started = time.time()
    cmd = [
        "cargo", "run", "--release", "--quiet", "--bin", "an-eval", "--",
        config, dataset, "--variant", variant, "--label", f"FT-{variant}",
    ]
    env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", "target"))
    print(f"\n===== {variant}：{VARIANTS[variant]} =====", flush=True)
    code = subprocess.call(cmd, env=env)
    if code != 0:
        print(f"[{variant}] an-eval 退出码 {code}，跳过", file=sys.stderr)
        return None
    # 取本组刚生成的 summary（修改时间晚于开始时间）
    files = [
        f for f in glob.glob(f"evals/results/*-FT-{variant}-*.summary.json")
        if os.path.getmtime(f) >= started - 1
    ]
    if not files:
        print(f"[{variant}] 没找到结果文件", file=sys.stderr)
        return None
    latest = max(files, key=os.path.getmtime)
    with open(latest, encoding="utf-8") as f:
        summary = json.load(f)
    summary["_file"] = latest
    summary["_wall_secs"] = round(time.time() - started)
    return summary


def pct(x):
    """an-eval 的 rate 已经是百分数（例如 85.3）"""
    return "-" if x is None else f"{x:.1f}%"


def rate(summary, key):
    """an-eval 的比率字段是 {"rate": .., "ok": .., "total": ..} 或 null"""
    v = summary.get(key)
    if isinstance(v, dict):
        return v.get("rate")
    return v if isinstance(v, (int, float)) else None


def report(results):
    lines = [
        "# FT 前台提速实验结果",
        "",
        f"生成时间：{time.strftime('%Y-%m-%d %H:%M:%S')}。"
        f"验收：整体准确率 ≥ {TARGET_PASS * 100:.1f}%，整轮 P95 ≤ {TARGET_P95_MS} ms（准确率优先）。",
        "",
        "| 组 | 配置 | 整体 | 动作 | 角色 | 类别 | 风险 | 判断 P50/P95 | 路由 P50/P95 | 整轮 P50/P95 | 重判样本 | 耗时 |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for v, s in results.items():
        if s is None:
            lines.append(f"| {v} | {VARIANTS[v]} | 失败 | | | | | | | | | |")
            continue
        t, r, a = s.get("turn_ms", {}), s.get("route_ms", {}), s.get("total_ms", {})
        esc = s.get("escalated", {})
        lines.append(
            f"| {v} | {VARIANTS[v]} | {pct(rate(s, 'pass'))} | {pct(rate(s, 'action'))} | "
            f"{pct(rate(s, 'role'))} | {pct(rate(s, 'class'))} | {pct(rate(s, 'risk'))} | "
            f"{t.get('p50')}/{t.get('p95')} | {r.get('p50')}/{r.get('p95')} | "
            f"{a.get('p50')}/{a.get('p95')} | {esc.get('cases', 0)} {esc.get('reasons', {})} | "
            f"{s.get('_wall_secs')} s |"
        )
    lines += ["", "各组原始结果文件："]
    for v, s in results.items():
        if s:
            lines.append(f"- {v}：`{s['_file']}`")
    return "\n".join(lines) + "\n"


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--config", default="config.toml")
    p.add_argument("--dataset", default="evals/routing.jsonl")
    p.add_argument("--variants", nargs="+", default=list(VARIANTS), choices=list(VARIANTS))
    a = p.parse_args()
    if not os.path.exists(a.config):
        sys.exit(f"找不到配置文件 {a.config}（在仓库根目录运行，或用 --config 指定）")

    results = {v: run_variant(v, a.config, a.dataset) for v in a.variants}
    stamp = time.strftime("%Y%m%d-%H%M%S")
    os.makedirs("evals/results", exist_ok=True)
    md_path = f"evals/results/ft-report-{stamp}.md"
    with open(md_path, "w", encoding="utf-8") as f:
        f.write(report(results))
    with open(f"evals/results/ft-report-{stamp}.json", "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    print("\n" + report(results))
    print(f"对比表：{md_path}")


if __name__ == "__main__":
    main()
