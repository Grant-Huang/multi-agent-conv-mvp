"""风险分级：根据改动的文件路径和新增内容，计算 PR 的风险等级 L0~L3。

用法（在仓库根目录运行）：
    python3 tools/ci/risk_classify.py [--base origin/master] [--head HEAD] \
        [--policy config/risk-policy.toml] [--json-out risk.json]

设计要点（对应 docs/AUTONOMOUS_DEV_ACTION_GUIDE.md 的风险分类）：
- 等级由程序按规则计算，模型或作者的自述不参与决定。
- 未匹配任何规则的文件使用 default_level（默认 L2），而不是被忽略：
  新增目录忘记登记时，宁可多审也不漏审。
- 内容信号只检查新增行，命中后把对应文件至少提到信号等级。
- 退出码：0 = 计算成功（无论等级高低）；2 = 配置或 git 错误。
  等级是否允许合并由分支保护 + CODEOWNERS 强制，本脚本只给出结论与理由。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

# 直接以脚本方式运行时，把本目录加入 sys.path 以便导入同目录的 common。
sys.path.insert(0, str(Path(__file__).resolve().parent))

from common import (  # noqa: E402  (需要先修改 sys.path)
    changed_files,
    diff_text,
    path_matches,
    resolve_base,
)

LEVEL_ORDER = ["L0", "L1", "L2", "L3"]
VALID_LEVELS = set(LEVEL_ORDER)


def higher(a: str, b: str) -> str:
    """返回两个等级中较高的一个。"""
    return a if LEVEL_ORDER.index(a) >= LEVEL_ORDER.index(b) else b


class PolicyError(ValueError):
    """策略文件格式错误。"""


@dataclass
class FileRisk:
    path: str
    level: str
    reasons: list[str] = field(default_factory=list)


def load_policy(path: str | Path) -> dict:
    """读取并校验策略文件；格式错误直接抛出 PolicyError，避免带着错误配置放行。"""
    with open(path, "rb") as f:
        data = tomllib.load(f)

    default = data.get("default_level", "L2")
    if default not in VALID_LEVELS:
        raise PolicyError(f"default_level 不合法：{default}")

    for i, rule in enumerate(data.get("rules", [])):
        if rule.get("level") not in VALID_LEVELS:
            raise PolicyError(f"rules[{i}].level 不合法：{rule.get('level')}")
        if not rule.get("paths"):
            raise PolicyError(f"rules[{i}] 缺少 paths")

    for i, sig in enumerate(data.get("signals", [])):
        if sig.get("level") not in VALID_LEVELS:
            raise PolicyError(f"signals[{i}].level 不合法：{sig.get('level')}")
        try:
            # 预编译正则，写错时在策略加载阶段就报错，而不是等到运行时。
            re.compile(sig["pattern"])
        except (KeyError, re.error) as e:
            raise PolicyError(f"signals[{i}].pattern 无效：{e}") from e

    return data


def classify_path(path: str, rules: list[dict], default: str) -> FileRisk:
    """按路径规则计算单个文件的等级：取所有匹配规则中最高的一条。"""
    level = None
    reasons: list[str] = []
    for rule in rules:
        if any(path_matches(path, pat) for pat in rule["paths"]):
            level = rule["level"] if level is None else higher(level, rule["level"])
            reasons.append(rule.get("reason", rule["level"]))
    if level is None:
        # 未登记的文件：使用默认等级，并在理由中注明，方便人工补登记。
        return FileRisk(path, default, [f"未匹配任何规则，使用默认等级 {default}"])
    return FileRisk(path, level, reasons)


def added_lines_of(diff: str) -> dict[str, list[str]]:
    """从 `git diff --unified=0` 的输出中提取每个文件的新增行（不含 +++ 头）。"""
    result: dict[str, list[str]] = {}
    current: str | None = None
    in_hunk = False  # 进入 @@ 之后，以 + 开头的行都是内容，即使它看起来像 "+++ "
    for line in diff.splitlines():
        if line.startswith("diff --git "):
            in_hunk = False
            current = None
        elif line.startswith("@@"):
            in_hunk = True
        elif not in_hunk and line.startswith("+++ "):
            # 文件头，形如 "+++ b/crates/x.rs"；删除文件时为 "+++ /dev/null"。
            target = line[4:]
            current = target[2:] if target.startswith("b/") else None
            if current is not None:
                result.setdefault(current, [])
        elif in_hunk and line.startswith("+") and current is not None:
            result[current].append(line[1:])
    return result


def signal_hits(path: str, added: list[str], signals: list[dict]) -> list[tuple[str, str]]:
    """返回 (信号名, 等级) 列表：路径匹配且新增行命中正则的信号。"""
    hits = []
    for sig in signals:
        if not any(path_matches(path, pat) for pat in sig.get("paths", ["**"])):
            continue
        regex = re.compile(sig["pattern"])
        if any(regex.search(line) for line in added):
            hits.append((sig["name"], sig["level"]))
    return hits


def evaluate(
    paths: list[str],
    added_by_path: dict[str, list[str]],
    policy: dict,
) -> dict:
    """纯函数：根据文件列表、新增行和策略计算整个 PR 的等级。便于单元测试。"""
    default = policy.get("default_level", "L2")
    rules = policy.get("rules", [])
    signals = policy.get("signals", [])

    files: list[FileRisk] = []
    for path in sorted(set(paths)):
        risk = classify_path(path, rules, default)
        for name, level in signal_hits(path, added_by_path.get(path, []), signals):
            # 信号只能提高等级，不能降低；理由也要记录下来。
            risk.level = higher(risk.level, level)
            risk.reasons.append(f"内容信号：{name}（{level}）")
        files.append(risk)

    overall = default if not files else max((f.level for f in files), key=LEVEL_ORDER.index)
    approvals = policy.get("approvals", {})
    return {
        "level": overall,
        "approval": approvals.get(overall, ""),
        "files": [
            {"path": f.path, "level": f.level, "reasons": f.reasons} for f in files
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="计算 PR 的风险等级")
    parser.add_argument("--base", help="比较基线，默认 origin/$GITHUB_BASE_REF 或 origin/master")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--policy", default="config/risk-policy.toml")
    parser.add_argument("--json-out", help="把结果写入该 JSON 文件")
    args = parser.parse_args(argv)

    try:
        policy = load_policy(args.policy)
        base = resolve_base(args.base)
        changes = changed_files(base, args.head)
        # 只取新增行用于内容信号；无改动时不需要读 diff。
        added = added_lines_of(diff_text(base, args.head)) if changes else {}
    except (PolicyError, OSError, RuntimeError, tomllib.TOMLDecodeError) as e:
        print(f"[risk_classify] 错误：{e}", file=sys.stderr)
        return 2

    result = evaluate([c.path for c in changes], added, policy)
    result["base"] = base
    result["head"] = args.head

    print(f"风险等级：{result['level']}（{result['approval']}）")
    for f in result["files"]:
        if LEVEL_ORDER.index(f["level"]) >= LEVEL_ORDER.index("L2"):
            # 只把 L2 及以上的文件逐条打印，避免 L0/L1 刷屏。
            print(f"  {f['level']}  {f['path']}：{'；'.join(f['reasons'])}")
    if not changes:
        print("  （与基线相比没有改动）")

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as f:
            json.dump(result, f, ensure_ascii=False, indent=2)

    # GitHub Actions 中输出摘要，方便在 PR 页面直接看到等级。
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(f"### 风险等级：{result['level']}\n\n{result['approval']}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
