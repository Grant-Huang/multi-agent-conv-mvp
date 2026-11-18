"""请求/响应数据模型。

约定：响应对象（EvaluateResult / RestoreResult 以外的审计结构）不携带实体原文；
只有 sanitized_* 字段承载外发内容，原值只存在于加密 Vault。
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
from typing import Any

# 四种决策，与设计文档第 1.1 节一致
ALLOW = "ALLOW"
ANONYMIZE = "ANONYMIZE"
REVIEW = "REVIEW"
BLOCK = "BLOCK"

# 接口版本（D1 定稿：返回中带版本号，工具名不变，按版本做兼容）
API_VERSION = "1.0"
# 通道固定取值（D1 定稿 9.3）。agent = 发给外部助手（AG.5 第 4 层）
CHANNELS = ("email", "slack", "external_api", "file_upload", "mcp", "agent")
# 单次载荷上限（D1 定稿 9.7）：按 UTF-8 字节数计，超出即 BLOCK
MAX_PAYLOAD_BYTES = 256 * 1024

# 决策严重程度，用于“取最严”合并
DECISION_RANK = {ALLOW: 0, ANONYMIZE: 1, REVIEW: 2, BLOCK: 3}


@dataclass
class EvaluateRequest:
    """一次外发评估请求。

    tenant_id / actor_id 必须来自可信身份上下文（MCP 服务端配置、mTLS、JWT），
    不能由 Agent 在工具参数里自报；MCP 服务在 mcp_server.py 中强制这一点。
    text 与 json_body 二选一：text 为自由文本，json_body 为结构化工具参数。
    """
    tenant_id: str
    actor_id: str
    task_id: str
    channel: str
    destination_id: str
    purpose: str
    text: str | None = None
    json_body: Any = None
    constraints: list[str] = field(default_factory=list)
    language: str | None = None  # 为空时自动判断 zh / en


@dataclass
class DetectedEntity:
    """识别出的实体。只记录位置与来源，不记录原文，便于直接写入审计。"""
    type: str
    start: int
    end: int
    score: float
    source: str  # 识别器来源：rule / master_data / detect_secrets / gliner / presidio
    field_path: str = ""  # JSON 载荷中的字段路径，如 items[0].order；文本载荷为空


@dataclass
class EvaluateResult:
    decision: str
    policy_version: str
    sanitized_text: str | None = None
    sanitized_json: Any = None
    entities: list[DetectedEntity] = field(default_factory=list)
    reason_codes: list[str] = field(default_factory=list)
    tokens: list[str] = field(default_factory=list)  # 本次使用的 Token，不含原值
    request_id: str = ""  # 本次评估的请求 ID，用于审计关联（D1 定稿 9.4，原 vault_ref）
    validation: dict[str, bool] = field(default_factory=dict)
    ready_to_send: bool = False
    elapsed_ms: float = 0.0

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass
class RestoreResult:
    text: str
    restored: int
    unknown_tokens: list[str] = field(default_factory=list)
    refused_reason: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)
