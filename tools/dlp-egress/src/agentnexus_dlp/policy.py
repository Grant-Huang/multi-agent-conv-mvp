"""策略引擎：根据 目的地信任等级 + 实体类型 + 调用方约束 计算每个实体的动作与整体决策。

刻意保持为“版本化 YAML + 严格求值器”（设计文档 8.1），规则缺失时默认取最严动作（fail closed）。
"""
from __future__ import annotations

from dataclasses import dataclass, field
from importlib import resources
from pathlib import Path
from typing import Any

import yaml

from .models import ALLOW, ANONYMIZE, BLOCK, DECISION_RANK, REVIEW

BLOCK_SECRET = "BLOCK_SECRET"
REDACT_REVIEW = "REDACT_REVIEW"
TOKENIZE = "TOKENIZE"
PRESERVE = "PRESERVE"

ACTION_DECISION = {BLOCK_SECRET: BLOCK, REDACT_REVIEW: REVIEW, TOKENIZE: ANONYMIZE, PRESERVE: ALLOW}
VALID_ACTIONS = set(ACTION_DECISION)


@dataclass
class Destination:
    destination_id: str
    trust: str  # external_untrusted / internal
    purposes: list[str] = field(default_factory=list)  # 为空表示不限制用途
    allowed_recipient_domains: list[str] = field(default_factory=list)  # 为空表示不限制收件域


@dataclass
class Policy:
    version: str
    trust_levels: dict[str, dict[str, str]]
    preserve_on_request: dict[str, list[str]]
    never_preserve: set[str]
    allowed_restore_contexts: list[str]
    routing_keys: set[str]

    @classmethod
    def load(cls, path: str | Path | None = None) -> "Policy":
        if path:
            raw = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
        else:
            raw = yaml.safe_load(resources.files("agentnexus_dlp").joinpath("default_policy.yaml").read_text("utf-8"))
        return cls.from_dict(raw)

    @classmethod
    def from_dict(cls, raw: dict[str, Any]) -> "Policy":
        trust_levels = raw.get("trust_levels") or {}
        for level, table in trust_levels.items():
            for etype, action in table.items():
                if action not in VALID_ACTIONS:
                    raise ValueError(f"策略错误：{level}.{etype} 的动作 {action!r} 无效")
            # 凭证在任何信任等级下都必须阻断，配置错误直接拒绝加载
            if table.get("SECRET") != BLOCK_SECRET:
                raise ValueError(f"策略错误：信任等级 {level} 必须对 SECRET 使用 BLOCK_SECRET")
        return cls(
            version=str(raw["policy_version"]),
            trust_levels=trust_levels,
            preserve_on_request={k: list(v) for k, v in (raw.get("preserve_on_request") or {}).items()},
            never_preserve=set(raw.get("never_preserve") or ["SECRET", "INTERNAL_INFO"]) | {"SECRET"},
            allowed_restore_contexts=list(raw.get("allowed_restore_contexts") or []),
            routing_keys={k.lower() for k in raw.get("routing_keys") or []},
        )

    def entity_action(self, trust: str, etype: str, constraints: list[str]) -> str:
        """单个实体的动作。未知实体类型或未知信任等级 → 最严（REVIEW / BLOCK）。"""
        table = self.trust_levels.get(trust)
        if table is None:
            return BLOCK_SECRET
        if etype == "SECRET":
            return BLOCK_SECRET
        action = table.get(etype, REDACT_REVIEW)  # 没有配置的实体类型默认转人工
        if action == TOKENIZE and f"preserve:{etype}" in constraints and etype not in self.never_preserve:
            if trust in self.preserve_on_request.get(etype, []):
                action = PRESERVE
        return action


def combine(decisions: list[str]) -> str:
    """多个决策取最严。空列表 = 无敏感实体 = ALLOW。"""
    return max(decisions, key=lambda d: DECISION_RANK[d]) if decisions else ALLOW


def load_destinations(raw: dict[str, Any] | str | Path | None) -> dict[str, Destination]:
    """目的地注册表：dict 或 YAML 文件路径。未登记的目的地在服务层一律 BLOCK。"""
    if raw is None:
        return {}
    if isinstance(raw, (str, Path)):
        raw = yaml.safe_load(Path(raw).read_text(encoding="utf-8")) or {}
        raw = raw.get("destinations", raw)
    out = {}
    for dest_id, spec in raw.items():
        trust = spec.get("trust")
        if trust not in ("external_untrusted", "internal"):
            raise ValueError(f"目的地 {dest_id} 的 trust 必须是 external_untrusted 或 internal")
        out[dest_id] = Destination(dest_id, trust, list(spec.get("purposes") or []),
                                   [d.lower() for d in spec.get("allowed_recipient_domains") or []])
    return out


__all__ = ["Policy", "Destination", "combine", "load_destinations", "BLOCK_SECRET", "REDACT_REVIEW",
           "TOKENIZE", "PRESERVE", "ACTION_DECISION", "ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"]
