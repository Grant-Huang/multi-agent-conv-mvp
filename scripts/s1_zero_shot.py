#!/usr/bin/env python3
"""s1-llm-auto-router 在路由评测集上的零样本对照（离线，只读）。

目的：判断开源决策小模型 s1 的七个问题里，哪些能当本项目路由的输入信号。
不改任何代码、配置和数据集；不调用 llama-server；不对外发送评测内容
（只从 Hugging Face 下载一次模型，之后完全本地运行）。

对照三组（期望值来自 evals/routing.jsonl 的 reviewer 模式标注）：
  A. needs_tools          ↔ 类别是否为 L2/L4（"需要取数工具"那一半）
  B. category             ↔ 角色是否为 coding
  C. stakes               ↔ 风险等级（只列交叉表，不强行映射：两者含义不同）
另外记录：follow_up / difficulty 与动作的关系、置信度能否把错误排到低置信一端、
输入是否被截断（512 token）、单次判断延迟。

用法见 evals/S1_ZERO_SHOT.md。

    python3 scripts/s1_zero_shot.py                   # 默认：4 线程，延迟重复 3 遍
    python3 scripts/s1_zero_shot.py --label busy      # 35B 同时在跑时再测一次延迟

输出（evals/results/ 已在 .gitignore 中）：
    evals/results/s1-zero-shot-<时间>-<标签>.jsonl   每条样本的全部概率和耗时
    evals/results/s1-zero-shot-<时间>-<标签>.md      报告（发回这个和 .jsonl 即可）
    evals/results/s1-zero-shot-<时间>-<标签>.json    报告里的数字
"""

import argparse
import hashlib
import json
import os
import platform
import statistics
import sys
import time
from collections import Counter, defaultdict

# 固定模型版本：模型卡 2026-10-15 的提交。换版本时同时改这里和指南。
REPO_ID = "decisionmodels/s1-llm-auto-router"
REVISION = "572c5b6cdb53b77fbb26ed33568ebbc5b71c69c0"

# 七个问题的选项顺序，和模型卡 example.py 一致（输出头的顺序也按这个）。
CATEGORIES = ["coding", "agentic", "math", "knowledge", "long_context",
              "tool_use", "design", "summarisation", "general"]
DIFFICULTY = ["trivial", "easy", "moderate", "hard", "frontier"]
STAKES = ["negligible", "low", "medium", "high"]
FLAGS = ["needs_tools", "needs_vision", "needs_long_context", "follow_up"]

NEW_CONV = "(new conversation)"  # 模型卡约定：没有上下文时的占位文本
CTX_CHARS = 600                  # 模型卡：上下文只用前 600 个字符
MAX_TOKENS = 512                 # 模型卡：输入总长截断到 512 token

# A 组门槛：E1（关思考）的类别准确率。n=28 时 95% 区间约 ±16 个百分点，只作粗筛。
GATE_CLASS = 0.75


# ───────────────────────── 数据集 ─────────────────────────

def load_dataset(path):
    """读 routing.jsonl：跳过 // 注释行和空行。"""
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            s = line.strip()
            if s.startswith("{"):
                rows.append(json.loads(s))
    return rows


def build_context(case):
    """把对话历史和进行中的任务压成一段短摘要，作为 s1 的 context。

    s1 只看前 600 个字符，所以按"最近的在前"拼接，保证最相关的部分不被截掉。
    这是本脚本的取舍，不是模型卡的规定；报告里会注明。
    """
    parts = []
    for t in case.get("active_tasks") or []:
        parts.append(f"进行中的任务[{t.get('id')}]：{t.get('goal')}（{t.get('status')}）")
    for m in reversed(case.get("history") or []):
        who = "用户" if m.get("role") == "user" else "助手"
        parts.append(f"{who}：{m.get('content')}")
    ctx = "\n".join(parts)
    return ctx[:CTX_CHARS] if ctx else NEW_CONV


# ───────────────────────── 模型推理 ─────────────────────────

def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class S1Runner:
    """加载 ONNX 模型并对单条请求给出七个答案的完整概率分布。"""

    def __init__(self, model_dir, threads):
        # 延迟导入：只做分析（--reanalyze）时不需要这些依赖。
        import numpy as np
        import onnxruntime as ort
        from tokenizers import Tokenizer

        self.np = np
        meta = json.load(open(os.path.join(model_dir, "meta.json"), encoding="utf-8"))
        self.temps = meta["temperatures"]  # 每个输出头的校准温度
        tok_path = os.path.join(model_dir, "tokenizer", "tokenizer.json")
        # 推理用的分词器：和模型卡 example.py 完全一致（512，only_second 只截 request）。
        self.tok = Tokenizer.from_file(tok_path)
        self.tok.enable_truncation(MAX_TOKENS, strategy="only_second")
        # 另一个不截断的分词器，只用来记录真实长度，判断是否被截断（不计入延迟）。
        self.tok_full = Tokenizer.from_file(tok_path)
        self.tok_full.no_truncation()
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads  # 模型卡的延迟是 4 线程测的
        so.inter_op_num_threads = 1
        onnx_path = os.path.join(model_dir, "model.gq.onnx")
        self.sess = ort.InferenceSession(onnx_path, so, providers=["CPUExecutionProvider"])
        self.model_sha256 = sha256_of(onnx_path)
        self.ort_version = ort.__version__

    def classify(self, request, context):
        np = self.np
        n_tokens = len(self.tok_full.encode(context, request).ids)  # 真实长度，不计时
        t0 = time.perf_counter()
        ids = self.tok.encode(context, request).ids  # 与模型卡相同的截断
        arr = np.asarray([ids], dtype=np.int64)
        cat, dif, stk, flg = self.sess.run(None, {"input_ids": arr, "attention_mask": np.ones_like(arr)})
        ms = (time.perf_counter() - t0) * 1000  # 含分词 + 推理，不含后处理

        def softmax(x, t):
            z = x / t
            e = np.exp(z - z.max())
            return e / e.sum()

        pc = softmax(cat[0], self.temps["category"])
        pd = softmax(dif[0], self.temps["difficulty"])
        ps = softmax(stk[0], self.temps["stakes"])
        pf = 1 / (1 + np.exp(-flg[0] / self.temps["flags"]))
        return {
            "category": dict(zip(CATEGORIES, map(float, pc))),
            "difficulty": dict(zip(DIFFICULTY, map(float, pd))),
            "stakes": dict(zip(STAKES, map(float, ps))),
            "flags": dict(zip(FLAGS, map(float, pf))),
            "n_tokens": n_tokens,
            "truncated": n_tokens > MAX_TOKENS,
            "ms": ms,
        }


# ───────────────────────── 统计工具 ─────────────────────────

def argmax(d):
    return max(d, key=d.get)


def pct(xs, q):
    """最近秩法分位数（和 an-eval 一致的简单口径）。"""
    s = sorted(xs)
    return s[min(len(s) - 1, int(q * len(s)))] if s else None


def auroc(scores, labels):
    """AUROC（Mann-Whitney U，平局记 0.5）。正类或负类为空时返回 None。"""
    pos = [s for s, y in zip(scores, labels) if y]
    neg = [s for s, y in zip(scores, labels) if not y]
    if not pos or not neg:
        return None
    win = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg)
    return win / (len(pos) * len(neg))


def acc(pairs):
    """pairs: [(预测, 真值)] → (对的条数, 总数, 比例)。"""
    ok = sum(p == t for p, t in pairs)
    return ok, len(pairs), (ok / len(pairs) if pairs else None)


# ───────────────────────── 分析 ─────────────────────────

def analyze(rows):
    """rows：每条样本 {id, tags, expect..., pred}。返回报告用的数字。"""
    out = {}

    # A：needs_tools ↔ L2/L4。只用有类别期望的样本（coding/reviewer 不标类别）。
    a_rows = [r for r in rows if r.get("expect_class")]
    a_pairs, a_scores, a_truth, a_err, a_margin = [], [], [], [], []
    for r in a_rows:
        p = r["pred"]["flags"]["needs_tools"]
        truth = r["expect_class"] in ("L2", "L4")
        pred = p >= 0.5
        a_pairs.append((pred, truth))
        a_scores.append(p)
        a_truth.append(truth)
        a_err.append(pred != truth)
        a_margin.append(abs(p - 0.5))  # 离 0.5 越近越不确定
    by_class = defaultdict(list)
    for r in a_rows:
        by_class[r["expect_class"]].append(round(r["pred"]["flags"]["needs_tools"], 3))
    ok, n, rate = acc(a_pairs)
    out["A_needs_tools_vs_L2L4"] = {
        "ok": ok, "total": n, "rate": rate,
        "auroc": auroc(a_scores, a_truth),
        # 低置信是否对应错误：用"不确定度"(-margin) 预测"判错"的 AUROC，>0.5 说明有用
        "error_auroc_by_uncertainty": auroc([-m for m in a_margin], a_err),
        "needs_tools_by_class": dict(sorted(by_class.items())),
        "gate": GATE_CLASS,
        "pass_gate": rate is not None and rate >= GATE_CLASS,
    }

    # B：category ↔ 角色 coding。只用派任务且有角色期望的样本。
    b_rows = [r for r in rows if r.get("expect_action") == "delegate" and r.get("expect_role")]
    res_b = {}
    for name, cats in (("coding", {"coding"}), ("coding+design", {"coding", "design"})):
        pairs = [(argmax(r["pred"]["category"]) in cats, r["expect_role"] == "coding") for r in b_rows]
        scores = [sum(r["pred"]["category"][c] for c in cats) for r in b_rows]
        ok, n, rate = acc(pairs)
        res_b[name] = {"ok": ok, "total": n, "rate": rate,
                       "auroc": auroc(scores, [t for _, t in pairs])}
    xt = defaultdict(Counter)  # 期望角色 × s1 category
    for r in b_rows:
        xt[r["expect_role"]][argmax(r["pred"]["category"])] += 1
    conf = [r["pred"]["category"][argmax(r["pred"]["category"])] for r in b_rows]
    err = [(argmax(r["pred"]["category"]) == "coding") != (r["expect_role"] == "coding") for r in b_rows]
    res_b["error_auroc_by_uncertainty"] = auroc([-c for c in conf], err)
    res_b["crosstab_role_x_category"] = {k: dict(v) for k, v in sorted(xt.items())}
    out["B_category_vs_role_coding"] = res_b

    # C：stakes ↔ 风险。含义不同，只给交叉表和"期望档位"的均值。
    c_rows = [r for r in rows if r.get("expect_risk")]
    xt = defaultdict(Counter)
    ev = defaultdict(list)
    for r in c_rows:
        st = r["pred"]["stakes"]
        xt[r["expect_risk"]][argmax(st)] += 1
        ev[r["expect_risk"]].append(sum(i * st[k] for i, k in enumerate(STAKES)))
    out["C_stakes_vs_risk"] = {
        "crosstab_risk_x_stakes": {k: dict(v) for k, v in sorted(xt.items())},
        "mean_stakes_expected": {k: round(statistics.mean(v), 2) for k, v in sorted(ev.items())},
        "auroc_high_vs_rest": auroc(
            [sum(i * r["pred"]["stakes"][k] for i, k in enumerate(STAKES)) for r in c_rows],
            [r["expect_risk"] == "high" for r in c_rows]),
    }

    # D：动作相关的旁证（s1 没有"动作"问题，这里只看有没有可用信号）。
    out["D_action_signals"] = {
        "follow_up_auroc": auroc([r["pred"]["flags"]["follow_up"] for r in rows],
                                 [r["expect_action"] == "follow_up" for r in rows]),
        "delegate_auroc_by_needs_tools": auroc([r["pred"]["flags"]["needs_tools"] for r in rows],
                                               [r["expect_action"] == "delegate" for r in rows]),
        "crosstab_action_x_difficulty": {
            a: dict(Counter(argmax(r["pred"]["difficulty"]) for r in rows if r["expect_action"] == a))
            for a in sorted({r["expect_action"] for r in rows})},
    }

    # E：截断和延迟
    ms = [m for r in rows for m in r["pred"]["ms_all"]]
    out["E_runtime"] = {
        "truncated": [r["id"] for r in rows if r["pred"]["truncated"]],
        "tokens_p50": pct([r["pred"]["n_tokens"] for r in rows], 0.5),
        "tokens_max": max(r["pred"]["n_tokens"] for r in rows),
        "ms_p50": round(pct(ms, 0.5), 1),
        "ms_p95": round(pct(ms, 0.95), 1),
        "ms_max": round(max(ms), 1),
        "samples": len(ms),
        "deterministic": all(r["pred"]["deterministic"] for r in rows),
    }
    return out


def fmt(x):
    if x is None:
        return "—"
    return f"{x:.3f}" if isinstance(x, float) else str(x)


def render_md(meta, res, rows):
    a, b, c, d, e = (res[k] for k in ("A_needs_tools_vs_L2L4", "B_category_vs_role_coding",
                                      "C_stakes_vs_risk", "D_action_signals", "E_runtime"))
    L = [f"# s1 零样本对照：{meta['label']}", ""]
    L.append(f"时间 {meta['time']}；机器 {meta['machine']}；Python {meta['python']}；"
             f"onnxruntime {meta['ort']}；线程 {meta['threads']}；延迟重复 {meta['repeat']} 遍。")
    L.append(f"模型 {REPO_ID}@{REVISION[:8]}，model.gq.onnx sha256 `{meta['model_sha256']}`。")
    L.append(f"数据集 {meta['dataset']}（{len(rows)} 条，reviewer 模式期望）。"
             f"上下文：历史和进行中任务按最近优先拼接，取前 {CTX_CHARS} 字符。")
    if meta.get("note"):
        L.append(f"备注：{meta['note']}")
    L += ["", "## 结论速览", ""]
    verdict = "达到" if a["pass_gate"] else "未达到"
    L.append(f"- A 组 needs_tools 对 L2/L4：{a['ok']}/{a['total']} = {fmt(a['rate'])}，"
             f"{verdict}门槛 {GATE_CLASS:.0%}（E1 类别准确率；n 小，只作粗筛）。")
    L.append(f"- B 组 category=coding 对角色 coding：{fmt(b['coding']['rate'])}；"
             f"计入 design：{fmt(b['coding+design']['rate'])}。")
    L.append(f"- 延迟 P50 {e['ms_p50']} ms / P95 {e['ms_p95']} ms（{e['samples']} 次）；"
             f"截断 {len(e['truncated'])} 条；结果可复现：{e['deterministic']}。")
    L += ["", "## A. needs_tools ↔ 类别 L2/L4", ""]
    L.append(f"准确率（阈值 0.5）{fmt(a['rate'])}；AUROC {fmt(a['auroc'])}；"
             f"不确定度预测判错的 AUROC {fmt(a['error_auroc_by_uncertainty'])}。")
    L.append("")
    L.append("| 期望类别 | needs_tools 概率 |")
    L.append("|---|---|")
    for k, v in a["needs_tools_by_class"].items():
        L.append(f"| {k} | {', '.join(map(str, v))} |")
    L += ["", "## B. category ↔ 角色 coding", ""]
    for k in ("coding", "coding+design"):
        x = b[k]
        L.append(f"- 视为 coding 的类别 = {k}：{x['ok']}/{x['total']} = {fmt(x['rate'])}，AUROC {fmt(x['auroc'])}")
    L.append(f"- 置信度预测判错的 AUROC：{fmt(b['error_auroc_by_uncertainty'])}")
    L += ["", "| 期望角色 | " + " | ".join(CATEGORIES) + " |",
          "|---|" + "---|" * len(CATEGORIES)]
    for role, cnt in b["crosstab_role_x_category"].items():
        L.append(f"| {role} | " + " | ".join(str(cnt.get(cn, 0)) for cn in CATEGORIES) + " |")
    L += ["", "## C. stakes ↔ 风险（含义不同，仅供参考）", ""]
    L += ["| 期望风险 | " + " | ".join(STAKES) + " | stakes 期望档位均值 |",
          "|---|" + "---|" * (len(STAKES) + 1)]
    for risk, cnt in c["crosstab_risk_x_stakes"].items():
        L.append(f"| {risk} | " + " | ".join(str(cnt.get(s, 0)) for s in STAKES)
                 + f" | {c['mean_stakes_expected'][risk]} |")
    L.append(f"\nhigh 对其余的 AUROC：{fmt(c['auroc_high_vs_rest'])}")
    L += ["", "## D. 动作相关信号", ""]
    L.append(f"- follow_up 概率区分\"跟进\"的 AUROC：{fmt(d['follow_up_auroc'])}")
    L.append(f"- needs_tools 概率区分\"派任务\"的 AUROC：{fmt(d['delegate_auroc_by_needs_tools'])}")
    L += ["", "| 期望动作 | " + " | ".join(DIFFICULTY) + " |", "|---|" + "---|" * len(DIFFICULTY)]
    for act, cnt in d["crosstab_action_x_difficulty"].items():
        L.append(f"| {act} | " + " | ".join(str(cnt.get(x, 0)) for x in DIFFICULTY) + " |")
    L += ["", "## E. 运行情况", ""]
    L.append(f"- token 数 P50 {e['tokens_p50']}，最大 {e['tokens_max']}；超过 {MAX_TOKENS} 被截断的样本："
             f"{', '.join(e['truncated']) or '无'}")
    L.append(f"- 单次判断（分词 + 推理）P50 {e['ms_p50']} ms，P95 {e['ms_p95']} ms，最大 {e['ms_max']} ms")
    L.append("")
    return "\n".join(L)


# ───────────────────────── 主流程 ─────────────────────────

def run(args):
    if args.model_dir:
        model_dir = args.model_dir
    else:
        from huggingface_hub import snapshot_download
        model_dir = snapshot_download(REPO_ID, revision=REVISION)
    runner = S1Runner(model_dir, args.threads)
    cases = load_dataset(args.dataset)
    if args.limit:
        cases = cases[: args.limit]

    # 预热：首次推理要分配内存，单独丢掉，不计入延迟。
    for _ in range(5):
        runner.classify("预热", NEW_CONV)

    rows = []
    for i, case in enumerate(cases, 1):
        ctx = build_context(case)
        first = runner.classify(case["text"], ctx)
        ms_all = [first["ms"]]
        same = True
        # 重复推理只为测延迟；同时核对结果是否完全一致（应当一致）。
        for _ in range(args.repeat - 1):
            again = runner.classify(case["text"], ctx)
            ms_all.append(again["ms"])
            same = same and again["category"] == first["category"] and again["flags"] == first["flags"]
        first["ms_all"] = [round(m, 2) for m in ms_all]
        first["deterministic"] = same
        rows.append({
            "id": case["id"], "text": case["text"], "tags": case.get("tags", []),
            "context": ctx,
            "expect_action": case.get("expect_action"),
            "expect_role": case.get("expect_role"),
            "expect_class": case.get("expect_class"),
            "expect_tools": case.get("expect_tools"),
            "expect_risk": case.get("expect_risk"),
            "pred": first,
        })
        if i % 20 == 0:
            print(f"  {i}/{len(cases)}", flush=True)

    meta = {
        "label": args.label,
        "time": time.strftime("%Y-%m-%d %H:%M:%S"),
        "machine": f"{platform.platform()} / {platform.machine()} / {platform.processor() or '-'} / {os.cpu_count()} 核",
        "python": platform.python_version(),
        "ort": runner.ort_version,
        "threads": args.threads,
        "repeat": args.repeat,
        "model_sha256": runner.model_sha256,
        "dataset": args.dataset,
        "note": args.note,
    }
    return meta, rows


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--dataset", default="evals/routing.jsonl")
    p.add_argument("--model-dir", help="已下载好的模型目录；不给则按固定版本从 Hugging Face 下载")
    p.add_argument("--threads", type=int, default=4)
    p.add_argument("--repeat", type=int, default=3, help="每条推理几遍（只用于测延迟）")
    p.add_argument("--limit", type=int, default=0, help="只跑前 N 条（冒烟用）")
    p.add_argument("--label", default="idle", help="写进文件名，如 idle / busy")
    p.add_argument("--note", default="", help="写进报告的备注，如 35B 是否在跑")
    p.add_argument("--reanalyze", help="不跑模型，重新分析已有的 .jsonl（改分析口径时用）")
    args = p.parse_args()
    if args.repeat < 1:
        p.error("--repeat 至少为 1")

    if args.reanalyze:
        with open(args.reanalyze, encoding="utf-8") as f:
            lines = [json.loads(l) for l in f if l.strip()]
        meta, rows = lines[0]["meta"], lines[1:]
        base = os.path.splitext(args.reanalyze)[0] + "-reanalyzed"
    else:
        meta, rows = run(args)
        os.makedirs("evals/results", exist_ok=True)
        stamp = time.strftime("%Y%m%d-%H%M%S")
        base = f"evals/results/s1-zero-shot-{stamp}-{args.label}"
        with open(base + ".jsonl", "w", encoding="utf-8") as f:
            f.write(json.dumps({"meta": meta}, ensure_ascii=False) + "\n")  # 第一行是元信息
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")

    res = analyze(rows)
    with open(base + ".json", "w", encoding="utf-8") as f:
        json.dump({"meta": meta, "results": res}, f, ensure_ascii=False, indent=2)
    md = render_md(meta, res, rows)
    with open(base + ".md", "w", encoding="utf-8") as f:
        f.write(md)
    print(md)
    print(f"\n已写出：{base}.md / .json" + ("" if args.reanalyze else " / .jsonl"))


if __name__ == "__main__":
    sys.exit(main())
