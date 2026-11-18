"""外发评估服务：解析 → 识别 → 策略 → 转换 → 保真校验 → 审计（设计文档第 3 节）。

强制约定：
- Fail closed：任何异常、未登记目的地、空载荷、校验失败 → BLOCK，且不返回原文。
- 响应与审计日志中不出现实体原值、凭证或 Token 映射。
- evaluate 只给出判定与脱敏载荷，不负责实际发送；“发送”由受控连接器在 ready_to_send=True 时执行。
"""
from __future__ import annotations

import copy
import hashlib
import json
import logging
import os
import re
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from .detector import Detector
from .models import (ALLOW, ANONYMIZE, BLOCK, CHANNELS, MAX_PAYLOAD_BYTES, REVIEW, DetectedEntity,
                     EvaluateRequest, EvaluateResult, RestoreResult)
from .normalize import canonical, detect_language
from .policy import (ACTION_DECISION, BLOCK_SECRET, PRESERVE, REDACT_REVIEW, TOKENIZE, Policy, combine,
                     load_destinations)
from .vault import TOKEN_RE, TokenVault, decode_key

audit_log = logging.getLogger("agentnexus_dlp.audit")

BLOCKED_SECRET = "[BLOCKED_SECRET]"
REDACTED_CONFIDENTIAL = "[REDACTED_CONFIDENTIAL]"
_EMAIL_RE = re.compile(r"[A-Za-z0-9._%+\-]+@([A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,})")


@dataclass
class ServiceConfig:
    destinations: dict[str, Any] | str | Path | None = None
    master_data: dict[str, list[str]] | str | Path | None = None
    vault_path: str | Path = "dlp-vault.sqlite3"
    vault_key: bytes | str = b""
    token_ttl_seconds: int = 86400
    policy_path: str | Path | None = None
    enable_gliner: bool = False
    gliner_model: str | None = None
    gliner_threshold: float = 0.5
    max_payload_bytes: int = MAX_PAYLOAD_BYTES
    extra: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_env(cls, env: dict[str, str] | None = None) -> "ServiceConfig":
        """从环境变量构造配置（MCP 服务使用）。缺少密钥时直接报错，拒绝以不安全配置启动。"""
        env = dict(os.environ if env is None else env)
        key = env.get("DLP_VAULT_KEY")
        if not key and env.get("DLP_VAULT_KEY_FILE"):
            key = Path(env["DLP_VAULT_KEY_FILE"]).read_text(encoding="utf-8").strip()
        if not key:
            raise RuntimeError("缺少 DLP_VAULT_KEY 或 DLP_VAULT_KEY_FILE：拒绝在没有 Vault 密钥的情况下启动")
        return cls(
            destinations=env.get("DLP_DESTINATIONS_FILE"),
            master_data=env.get("DLP_MASTER_DATA_FILE"),
            vault_path=env.get("DLP_VAULT_PATH", "dlp-vault.sqlite3"),
            vault_key=key,
            token_ttl_seconds=int(env.get("DLP_TOKEN_TTL_SECONDS", "86400")),
            policy_path=env.get("DLP_POLICY_FILE") or None,
            enable_gliner=env.get("DLP_ENABLE_GLINER", "0") == "1",
            gliner_model=env.get("DLP_GLINER_MODEL") or None,
            gliner_threshold=float(env.get("DLP_GLINER_THRESHOLD", "0.5")),
        )


def _load_master_data(raw) -> dict[str, list[str]] | None:
    if raw is None or isinstance(raw, dict):
        return raw
    data = yaml.safe_load(Path(raw).read_text(encoding="utf-8")) or {}
    return {k: [str(x) for x in v] for k, v in data.items()}


@dataclass
class _Field:
    """载荷中的一个待处理字符串（文本载荷只有一个，JSON 载荷每个字符串值一个）。"""
    path: str
    text: str
    entities: list[DetectedEntity]
    actions: list[str]
    routing: list[bool]


class EgressService:
    def __init__(self, config: ServiceConfig):
        self.config = config
        self.policy = Policy.load(config.policy_path)
        self.destinations = load_destinations(config.destinations)
        self.detector = Detector(_load_master_data(config.master_data), enable_gliner=config.enable_gliner,
                                 gliner_model=config.gliner_model, gliner_threshold=config.gliner_threshold)
        self.vault = TokenVault(config.vault_path, decode_key(config.vault_key), config.token_ttl_seconds)

    def close(self) -> None:
        self.vault.close()

    # ------------------------------------------------------------------ evaluate
    def evaluate(self, req: EvaluateRequest) -> EvaluateResult:
        t0 = time.perf_counter()
        request_id = f"req_{uuid.uuid4().hex[:16]}"
        try:
            result = self._evaluate(req)
        except Exception as exc:  # noqa: BLE001 —— 任何异常都必须 fail closed
            logging.getLogger(__name__).error("evaluate 异常，按 BLOCK 处理: %s", type(exc).__name__)
            result = EvaluateResult(decision=BLOCK, policy_version=self.policy.version,
                                    reason_codes=["internal_error_fail_closed"], ready_to_send=False)
        result.elapsed_ms = round((time.perf_counter() - t0) * 1000, 3)
        result.request_id = request_id
        self._audit(request_id, req, result)
        return result

    def _blocked(self, *reasons: str) -> EvaluateResult:
        return EvaluateResult(decision=BLOCK, policy_version=self.policy.version, reason_codes=list(reasons))

    def _evaluate(self, req: EvaluateRequest) -> EvaluateResult:
        # 0. 通道必须是固定取值之一；载荷不得超过上限（D1 定稿 9.3、9.7）
        if req.channel not in CHANNELS:
            return self._blocked("invalid_channel")
        if _payload_bytes(req) > self.config.max_payload_bytes:
            return self._blocked("payload_too_large")
        # 1. 目的地与用途校验（先于内容检查）
        dest = self.destinations.get(req.destination_id)
        if dest is None:
            return self._blocked("destination_unknown")
        if dest.purposes and req.purpose not in dest.purposes:
            return self._blocked("purpose_not_allowed")
        if req.text is None and req.json_body is None:
            return self._blocked("empty_payload")
        if req.text is not None and req.json_body is not None:
            return self._blocked("ambiguous_payload")

        # 2. 识别 + 逐实体动作
        fields: list[_Field] = []
        if req.text is not None:
            fields.append(self._analyze_field("", req.text, dest, req, routing_ranges=_text_routing_ranges(
                req.text, self.policy.routing_keys)))
        else:
            self._collect_json(req.json_body, "", fields, dest, req)

        reasons: list[str] = []
        decisions = []
        for f in fields:
            for ent, action, is_routing in zip(f.entities, f.actions, f.routing):
                decisions.append(ACTION_DECISION[action])
                if is_routing and action == BLOCK_SECRET and ent.type == "EMAIL":
                    reasons.append("recipient_not_allowed")
        decision = combine(decisions)
        reasons += _reason_codes(fields)
        # 输入里已经带有 Token 形状的字符串：可能是再次外发已脱敏内容，也可能是 Token 注入，标记供审计
        if any(TOKEN_RE.search(f.text) for f in fields):
            reasons.append("token_like_input")

        # 3. 转换。BLOCK 时不写 Vault，非凭证实体只用类型占位，避免为不会发送的内容建立映射。
        tokens: list[str] = []
        write_vault = decision != BLOCK
        rendered = {f.path: self._render(f, req, write_vault, tokens) for f in fields}

        if req.text is not None:
            sanitized_text, sanitized_json = rendered[""], None
        else:
            sanitized_text, sanitized_json = None, _rebuild_json(req.json_body, "", rendered)

        # 4. 保真校验：不通过则不发送
        validation = _validate(req, fields, rendered, sanitized_json, self.policy.routing_keys)
        if not all(validation.values()):
            decision = BLOCK
            reasons.append("validation_failed")

        all_entities = [DetectedEntity(e.type, e.start, e.end, e.score, e.source, f.path)
                        for f in fields for e in f.entities]
        return EvaluateResult(
            decision=decision, policy_version=self.policy.version, sanitized_text=sanitized_text,
            sanitized_json=sanitized_json, entities=all_entities, reason_codes=sorted(set(reasons)),
            tokens=sorted(set(tokens)), validation=validation,
            ready_to_send=decision in (ALLOW, ANONYMIZE),
        )

    def _analyze_field(self, path: str, text: str, dest, req: EvaluateRequest, routing_ranges) -> _Field:
        ents = self.detector.detect(text, req.language or detect_language(text))
        actions, routing = [], []
        for e in ents:
            in_routing = any(s <= e.start and e.end <= t for s, t in routing_ranges)
            if in_routing and e.type == "EMAIL":
                # 收件地址是路由字段：校验是否允许，不做 Token 化
                ok = _recipient_allowed(text[e.start:e.end], dest.allowed_recipient_domains)
                actions.append(PRESERVE if ok else BLOCK_SECRET)
                routing.append(True)
            else:
                actions.append(self.policy.entity_action(dest.trust, e.type, req.constraints))
                routing.append(False)
        return _Field(path, text, ents, actions, routing)

    def _collect_json(self, node, path, fields, dest, req) -> None:
        """遍历 JSON：键名与非字符串值保持不变；路由键的值整体视为路由字段。"""
        if isinstance(node, dict):
            for k, v in node.items():
                child = f"{path}.{k}" if path else str(k)
                if str(k).lower() in self.policy.routing_keys:
                    for i, s in _iter_strings(v, child):
                        fields.append(self._analyze_field(i, s, dest, req, routing_ranges=[(0, len(s))]))
                else:
                    self._collect_json(v, child, fields, dest, req)
        elif isinstance(node, list):
            for i, v in enumerate(node):
                self._collect_json(v, f"{path}[{i}]", fields, dest, req)
        elif isinstance(node, str):
            fields.append(self._analyze_field(path, node, dest, req, routing_ranges=[]))

    def _render(self, f: _Field, req: EvaluateRequest, write_vault: bool, tokens: list[str]) -> str:
        # 先按出现顺序分配 Token（编号从左到右递增），再从右往左替换以保持偏移有效
        replacements = []
        for e, action in zip(f.entities, f.actions):
            original = f.text[e.start:e.end]
            if action == PRESERVE:
                continue
            if action == BLOCK_SECRET:
                rep = BLOCKED_SECRET
            elif action == REDACT_REVIEW:
                rep = REDACTED_CONFIDENTIAL
            elif write_vault:
                rep = self.vault.get_or_create(
                    req.tenant_id, req.task_id, e.type, original, canonical(original), req.actor_id,
                    self.policy.allowed_restore_contexts, self.policy.version)
                tokens.append(rep)
            else:
                rep = f"[{e.type}]"
            replacements.append((e.start, e.end, rep))
        out = f.text
        for start, end, rep in sorted(replacements, reverse=True):
            out = out[:start] + rep + out[end:]
        return out

    # ------------------------------------------------------------------ restore / revoke
    def restore(self, tenant_id: str, actor_id: str, task_id: str, text: str, context: str) -> RestoreResult:
        """把文本中属于本作用域的 Token 换回原值。外部响应里伪造/越界的 Token 原样保留。"""
        if context not in self.policy.allowed_restore_contexts:
            return RestoreResult(text=text, restored=0, refused_reason="context_not_allowed")
        restored = 0
        unknown: list[str] = []

        def sub(m: re.Match[str]) -> str:
            nonlocal restored
            token = m.group(0)
            value = self.vault.lookup(tenant_id, task_id, token, actor_id, context)
            if value is None:
                unknown.append(token)
                return token
            restored += 1
            return value

        out = TOKEN_RE.sub(sub, text)
        audit_log.info(json.dumps({"event": "dlp.restore", "tenant_id": tenant_id, "actor_id": actor_id,
                                   "task_id": task_id, "context": context, "restored": restored,
                                   "unknown": len(unknown)}, ensure_ascii=False))
        return RestoreResult(text=out, restored=restored, unknown_tokens=sorted(set(unknown)))

    def revoke(self, tenant_id: str, task_id: str) -> int:
        n = self.vault.revoke(tenant_id, task_id)
        audit_log.info(json.dumps({"event": "dlp.revoke", "tenant_id": tenant_id, "task_id": task_id,
                                   "revoked": n}))
        return n

    # ------------------------------------------------------------------ audit
    def _audit(self, request_id: str, req: EvaluateRequest, result: EvaluateResult) -> None:
        """审计只记录元数据：决策、原因、实体类型计数、载荷哈希、耗时。不记录正文与原值。"""
        body = req.text if req.text is not None else json.dumps(req.json_body, ensure_ascii=False, sort_keys=True)
        counts: dict[str, int] = {}
        for e in result.entities:
            counts[e.type] = counts.get(e.type, 0) + 1
        audit_log.info(json.dumps({
            "event": "dlp.evaluate", "request_id": request_id, "tenant_id": req.tenant_id,
            "actor_id": req.actor_id, "task_id": req.task_id, "channel": req.channel,
            "destination_id": req.destination_id, "purpose": req.purpose, "decision": result.decision,
            "reason_codes": result.reason_codes, "entity_counts": counts,
            "payload_sha256": hashlib.sha256((body or "").encode()).hexdigest(),
            "policy_version": result.policy_version, "elapsed_ms": result.elapsed_ms,
        }, ensure_ascii=False))


# ---------------------------------------------------------------------- helpers
def _payload_bytes(req: EvaluateRequest) -> int:
    """载荷大小（UTF-8 字节）。JSON 载荷按紧凑序列化计算。"""
    if req.text is not None:
        return len(req.text.encode("utf-8"))
    if req.json_body is not None:
        return len(json.dumps(req.json_body, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    return 0


def _text_routing_ranges(text: str, routing_keys: set[str]) -> list[tuple[int, int]]:
    """在工具调用文本里找路由参数的值区间，如 send_email(to="a@b.com") 中引号内的部分。"""
    if not routing_keys:
        return []
    keys = "|".join(sorted(map(re.escape, routing_keys), key=len, reverse=True))
    pattern = re.compile(rf"(?i)(?<![A-Za-z_])(?:{keys})\s*[=:]\s*(\[[^\]]*\]|\"[^\"]*\"|'[^']*')")
    return [m.span(1) for m in pattern.finditer(text)]


def _recipient_allowed(address: str, allowed_domains: list[str]) -> bool:
    if not allowed_domains:
        return True
    m = _EMAIL_RE.fullmatch(address)
    return bool(m) and m.group(1).lower() in allowed_domains


def _iter_strings(node, path):
    if isinstance(node, str):
        yield path, node
    elif isinstance(node, list):
        for i, v in enumerate(node):
            yield from _iter_strings(v, f"{path}[{i}]")


def _rebuild_json(node, path, rendered: dict[str, str]):
    if isinstance(node, dict):
        return {k: _rebuild_json(v, f"{path}.{k}" if path else str(k), rendered) for k, v in node.items()}
    if isinstance(node, list):
        return [_rebuild_json(v, f"{path}[{i}]", rendered) for i, v in enumerate(node)]
    if isinstance(node, str):
        return rendered.get(path, node)
    return copy.deepcopy(node)


def _reason_codes(fields: list[_Field]) -> list[str]:
    codes = set()
    for f in fields:
        for e, action, routing in zip(f.entities, f.actions, f.routing):
            if action == BLOCK_SECRET and e.type == "SECRET":
                codes.add("secret_blocked")
            elif action == REDACT_REVIEW:
                codes.add("confidential_requires_review")
            elif action == TOKENIZE:
                codes.add(f"{e.type.lower()}_tokenized")
            elif action == PRESERVE:
                codes.add("routing_preserved" if routing else f"{e.type.lower()}_preserved_by_policy")
    return sorted(codes)


def _same_shape(a, b) -> bool:
    """结构一致：键集合、列表长度、非字符串值都不变。"""
    if isinstance(a, dict):
        return isinstance(b, dict) and a.keys() == b.keys() and all(_same_shape(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return isinstance(b, list) and len(a) == len(b) and all(_same_shape(x, y) for x, y in zip(a, b))
    if isinstance(a, str):
        return isinstance(b, str)
    return a == b


def _validate(req, fields, rendered, sanitized_json, routing_keys) -> dict[str, bool]:
    preserved_ok = secrets_ok = routing_ok = True
    for f in fields:
        out = rendered[f.path]
        for e, action, routing in zip(f.entities, f.actions, f.routing):
            original = f.text[e.start:e.end]
            if action == PRESERVE and out.count(original) < f.text.count(original):
                preserved_ok = False  # 应保留的数值/单位/设备号/收件人被改动
                if routing:
                    routing_ok = False
            if e.type == "SECRET" and original in out:
                secrets_ok = False
    schema_ok = True
    if req.json_body is not None:
        schema_ok = _same_shape(req.json_body, sanitized_json)
        try:
            json.dumps(sanitized_json, ensure_ascii=False)
        except (TypeError, ValueError):
            schema_ok = False
    return {"schema_ok": schema_ok, "preserved_values_ok": preserved_ok, "no_secret_in_output": secrets_ok,
            "routing_unchanged": routing_ok}


__all__ = ["EgressService", "ServiceConfig", "ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"]
