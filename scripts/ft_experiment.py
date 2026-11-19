#!/usr/bin/env python3
"""FT 前台提速实验：按顺序跑若干组配置（默认 E0~E5），汇总成一份对比表。

用法（在仓库根目录、本机模型服务已启动时）：
    python3 scripts/ft_experiment.py                      # 用 config.toml 和 evals/routing.jsonl
    python3 scripts/ft_experiment.py --config my.toml --variants E0 E1 E4
    python3 scripts/ft_experiment.py --variants E1 E6 E0 E7 --repeat 3   # FT.5：每组跑 3 次

--repeat N 时按轮次交替运行（第 1 轮 E1 E6 E0 E7，第 2 轮再来一遍……），
让机器负载的变化平均落到各组上；报告里另给每组 N 次的均值和最小～最大值，用来判断差距是否超过波动。

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
    "E6": "关思考 + Q 类别拆成两个是/否问题（FT.5，对照 E1）",
    "E7": "开思考 + Q 类别拆成两个是/否问题（FT.5，对照 E0）",
}

# 不写 --variants 时跑的组（保持 FT.3 时的默认）
DEFAULT_VARIANTS = ["E0", "E1", "E2", "E3", "E4", "E5"]

# 验收标准（用户 2026-11-17 确认：准确率优先，P95 放宽到 6 秒）。
# 准确率门槛是"不低于同一次实验里 E0（开思考）的结果"，不再用 2026-11-05 旧评测集的 85.3%
TARGET_P95_MS = 6000


def run_variant(variant, config, dataset, label):
    """跑一组配置，返回它生成的 summary.json 内容；失败返回 None。"""
    started = time.time()
    cmd = [
        "cargo", "run", "--release", "--quiet", "--bin", "an-eval", "--",
        config, dataset, "--variant", variant, "--label", label,
    ]
    env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", "target"))
    print(f"\n===== {label}：{VARIANTS[variant]} =====", flush=True)
    code = subprocess.call(cmd, env=env)
    if code != 0:
        print(f"[{variant}] an-eval 退出码 {code}，跳过", file=sys.stderr)
        return None
    # 取本组刚生成的 summary（修改时间晚于开始时间）
    files = [
        f for f in glob.glob(f"evals/results/*-{label}-*.summary.json")
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
        f"验收：整体准确率不低于本次 E0（开思考）的结果，整轮 P95 ≤ {TARGET_P95_MS} ms（准确率优先）。"
        "100 条单次运行的抽样误差约 ±4 个百分点，相差 1～3 个百分点不足以下结论。",
        "",
        "| 组 | 配置 | 整体 | 动作 | 角色 | 类别 | 风险 | 判断 P50/P95 | 路由 P50/P95 | 整轮 P50/P95 | 重判样本 | 耗时 |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for key, s in results.items():
        v = key.split("#")[0]  # 重复运行时键为 "E1#2"
        if s is None:
            lines.append(f"| {key} | {VARIANTS[v]} | 失败 | | | | | | | | | |")
            continue
        t, r, a = s.get("turn_ms", {}), s.get("route_ms", {}), s.get("total_ms", {})
        esc = s.get("escalated", {})
        lines.append(
            f"| {key} | {VARIANTS[v]} | {pct(rate(s, 'pass'))} | {pct(rate(s, 'action'))} | "
            f"{pct(rate(s, 'role'))} | {pct(rate(s, 'class'))} | {pct(rate(s, 'risk'))} | "
            f"{t.get('p50')}/{t.get('p95')} | {r.get('p50')}/{r.get('p95')} | "
            f"{a.get('p50')}/{a.get('p95')} | {esc.get('cases', 0)} {esc.get('reasons', {})} | "
            f"{s.get('_wall_secs')} s |"
        )
    lines += spread(results)
    lines += ["", "各组原始结果文件："]
    for key, s in results.items():
        if s:
            lines.append(f"- {key}：`{s['_file']}`")
    return "\n".join(lines) + "\n"


def spread(results):
    """同一组重复运行时，给出均值和最小～最大值（只有一次运行的组不列）。"""
    groups = {}
    for key, s in results.items():
        if s is not None:
            groups.setdefault(key.split("#")[0], []).append(s)
    groups = {v: runs for v, runs in groups.items() if len(runs) > 1}
    if not groups:
        return []

    def stat(values):
        values = [x for x in values if x is not None]
        if not values:
            return "-"
        mean = sum(values) / len(values)
        return f"{mean:.1f}（{min(values):.1f}～{max(values):.1f}）"

    lines = [
        "",
        "## 重复运行的波动（均值，括号内为最小～最大）",
        "",
        "| 组 | 次数 | 整体 % | 类别 % | 角色 % | 风险 % | 整轮 P95 ms |",
        "|---|---|---|---|---|---|---|",
    ]
    for v, runs in groups.items():
        lines.append(
            f"| {v} | {len(runs)} | {stat([rate(s, 'pass') for s in runs])} | "
            f"{stat([rate(s, 'class') for s in runs])} | {stat([rate(s, 'role') for s in runs])} | "
            f"{stat([rate(s, 'risk') for s in runs])} | "
            f"{stat([s.get('total_ms', {}).get('p95') for s in runs])} |"
        )
    return lines


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--config", default="config.toml")
    p.add_argument("--dataset", default="evals/routing.jsonl")
    p.add_argument("--variants", nargs="+", default=DEFAULT_VARIANTS, choices=list(VARIANTS))
    p.add_argument("--repeat", type=int, default=1, help="每组跑几次（按轮次交替），用来估计波动")
    a = p.parse_args()
    if a.repeat < 1:
        p.error("--repeat 至少为 1")
    if not os.path.exists(a.config):
        sys.exit(f"找不到配置文件 {a.config}（在仓库根目录运行，或用 --config 指定）")

    results = {}
    for i in range(1, a.repeat + 1):
        for v in a.variants:
            # 只跑一次时键和标签与以前相同（E1 / FT-E1）；重复时加轮次（E1#2 / FT-E1-r2）
            key, label = (v, f"FT-{v}") if a.repeat == 1 else (f"{v}#{i}", f"FT-{v}-r{i}")
            results[key] = run_variant(v, a.config, a.dataset, label)
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
