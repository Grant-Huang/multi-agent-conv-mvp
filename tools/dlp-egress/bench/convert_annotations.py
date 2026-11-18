"""校验人工标注文件，并转换为评测脚本可直接使用的 JSONL。

输入：CSV（UTF-8，可带 BOM，即 Excel 导出的格式）或 JSONL，字段见 docs/ANNOTATION_GUIDE.md。
输出：与 700 条评测集同构的 JSONL，可直接用于 `python -m bench.run_benchmark --data <输出文件>`。

只要有一条错误，就不写输出文件（避免用半成品做评测）。
“分歧”指标注员给出的 expected_action 与按策略推导的决策不一致：默认视为需要复核的问题，退出码非 0；
复核后若确认是策略应当调整，可加 --allow-disagreement 先转换，分歧清单会写在报告里。

用法：
  uv run python -m bench.convert_annotations annotations.csv --out /tmp/new.jsonl
"""
from __future__ import annotations

import argparse
import csv
import json
import sys
from pathlib import Path

from bench.annotation import (ACTIONS, TRUST_LEVELS, AnnotationError, derive_expected, detect_language,
                              parse_markup)

CHANNELS = ("email", "slack", "external_api", "file_upload", "mcp")
REQUIRED = ("id", "language", "channel", "destination_trust", "annotated_text", "expected_action")


def _read(path: Path) -> list[dict]:
    if path.suffix.lower() == ".csv":
        with open(path, encoding="utf-8-sig", newline="") as f:
            return [dict(r) for r in csv.DictReader(f)]
    rows = []
    with open(path, encoding="utf-8-sig") as f:
        for n, line in enumerate(f, 1):
            if line.strip():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError as exc:
                    rows.append({"__error__": f"第 {n} 行不是合法 JSON：{exc}"})
    return rows


def _constraints(raw) -> list[str]:
    if isinstance(raw, list):
        return [str(x).strip() for x in raw if str(x).strip()]
    return [x.strip() for x in str(raw or "").replace("；", ";").split(";") if x.strip()]


def convert(src: str | Path, out: str | Path | None, allow_disagreement: bool = False) -> dict:
    rows = _read(Path(src))
    errors: list[dict] = []
    disagreements: list[dict] = []
    converted: list[dict] = []
    seen: set[str] = set()
    for idx, raw in enumerate(rows, 1):
        rid = str(raw.get("id", "")).strip() or f"<第{idx}条>"

        def err(msg: str, rid=rid) -> None:
            errors.append({"id": rid, "message": msg})

        if "__error__" in raw:
            err(raw["__error__"])
            continue
        missing = [k for k in REQUIRED if not str(raw.get(k, "")).strip()]
        if missing:
            err(f"缺少必填字段：{', '.join(missing)}")
            continue
        if rid in seen:
            err("id 重复")
            continue
        seen.add(rid)
        lang = str(raw["language"]).strip()
        channel = str(raw["channel"]).strip()
        trust = str(raw["destination_trust"]).strip()
        expected = str(raw["expected_action"]).strip().upper()
        bad = False
        if lang not in ("zh", "en"):
            err(f"language 只能是 zh 或 en，当前为 {lang}")
            bad = True
        if channel not in CHANNELS:
            err(f"channel 只能是 {'/'.join(CHANNELS)}，当前为 {channel}")
            bad = True
        if trust not in TRUST_LEVELS:
            err(f"destination_trust 只能是 {'/'.join(TRUST_LEVELS)}，当前为 {trust}")
            bad = True
        if expected not in ACTIONS:
            err(f"expected_action 只能是 {'/'.join(ACTIONS)}，当前为 {expected}")
            bad = True
        constraints = _constraints(raw.get("constraints"))
        for c in constraints:
            if not c.startswith("preserve:"):
                err(f"constraints 目前只支持 preserve:<TYPE>，当前为 {c}")
                bad = True
        try:
            source, entities = parse_markup(str(raw["annotated_text"]))
        except AnnotationError as exc:
            err(str(exc))
            continue
        if bad:
            continue
        if detect_language(source) != lang and lang == "en":
            err("language 标为 en，但文本中含中文；中英混排请标 zh")
            continue
        policy_action, sanitized = derive_expected(source, entities, trust, constraints)
        if policy_action != expected:
            disagreements.append({"id": rid, "annotator_action": expected, "policy_action": policy_action,
                                  "annotated_text": raw["annotated_text"]})
        converted.append({
            "id": rid, "language": lang, "domain": str(raw.get("domain", "") or "manual"), "channel": channel,
            "scenario": str(raw.get("scenario", "") or ""), "difficulty": str(raw.get("difficulty", "") or ""),
            "source_text": source, "entities": entities, "expected_action": expected,
            "policy_action": policy_action, "expected_sanitized_text": sanitized,
            "destination_trust": trust, "purpose": str(raw.get("purpose", "") or ""),
            "constraints": constraints, "annotator": str(raw.get("annotator", "") or ""),
            "reviewer": str(raw.get("reviewer", "") or ""), "is_synthetic": True,
        })
    blocked = bool(errors) or (bool(disagreements) and not allow_disagreement)
    written = 0
    if out is not None and not blocked:
        with open(out, "w", encoding="utf-8") as f:
            for r in converted:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
        written = len(converted)
    return {"total": len(rows), "valid": len(converted), "written": written, "errors": errors,
            "disagreements": disagreements, "stats": _stats(converted)}


def _stats(rows: list[dict]) -> dict:
    from collections import Counter
    return {
        "language": dict(Counter(r["language"] for r in rows)),
        "channel": dict(Counter(r["channel"] for r in rows)),
        "expected_action": dict(Counter(r["expected_action"] for r in rows)),
        "destination_trust": dict(Counter(r["destination_trust"] for r in rows)),
        "difficulty": dict(Counter(r["difficulty"] or "未填" for r in rows)),
        "entity_type": dict(Counter(e["type"] for r in rows for e in r["entities"])),
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="校验并转换人工标注文件")
    ap.add_argument("src", help="标注文件（.csv 或 .jsonl）")
    ap.add_argument("--out", help="输出 JSONL；不填则只校验")
    ap.add_argument("--allow-disagreement", action="store_true", help="存在分歧时仍然输出")
    args = ap.parse_args(argv)
    rep = convert(args.src, args.out, args.allow_disagreement)
    print(f"共 {rep['total']} 条，合格 {rep['valid']} 条，写出 {rep['written']} 条")
    for e in rep["errors"]:
        print(f"[错误] {e['id']}：{e['message']}")
    for d in rep["disagreements"]:
        print(f"[分歧] {d['id']}：标注 {d['annotator_action']}，按策略应为 {d['policy_action']}｜{d['annotated_text']}")
    print("分布：" + json.dumps(rep["stats"], ensure_ascii=False))
    if rep["errors"]:
        return 1
    if rep["disagreements"] and not args.allow_disagreement:
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main())
