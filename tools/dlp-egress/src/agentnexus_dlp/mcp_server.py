"""stdio MCP 服务：把外发脱敏服务提供给 AgentNexus（或任何 MCP 客户端）。

身份边界：
- tenant_id / actor_id 只从服务端配置（DLP_TENANT_ID / DLP_ACTOR_ID）读取，工具参数里没有这两个字段。
- AgentNexus 侧由 Rust 宿主直接调用这三个工具，不交给模型（D1 定稿 9.2），所以 task_id 由编排器提供。
- egress_restore 默认关闭（DLP_ALLOW_RESTORE=0），只允许 local_ui 场景（D1 定稿 9.6）。

返回格式（D1 定稿 9.1、9.8、9.9）：结构化返回 + output schema，统一外壳
  {"api_version": "1.0", "status": "success"|"error", "data": {...}|null, "error": {code, message}|null}
同时以文本内容返回同一份 JSON，兼容不支持结构化返回的客户端。

stdout 留给 MCP 协议，日志（含审计日志）只写 stderr。
"""
from __future__ import annotations

import logging
import os
import sys
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict

from .models import API_VERSION, EvaluateRequest

INSTALL_HINT = "未安装 MCP 支持，请安装可选依赖：pip install 'agentnexus-dlp[mcp]'"


# ---------------------------------------------------------------- 输出 schema（Pydantic → JSON Schema）
class _Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class ErrorInfo(_Strict):
    code: str
    message: str


class EntityOut(_Strict):
    type: str
    start: int
    end: int
    score: float
    source: str
    field_path: str


class EvaluateData(_Strict):
    decision: Literal["ALLOW", "ANONYMIZE", "REVIEW", "BLOCK"]
    policy_version: str
    request_id: str
    sanitized_text: str | None
    sanitized_json: Any
    entities: list[EntityOut]
    reason_codes: list[str]
    tokens: list[str]
    validation: dict[str, bool]
    ready_to_send: bool
    elapsed_ms: float


class RestoreData(_Strict):
    text: str
    restored: int
    unknown_tokens: list[str]
    refused_reason: str | None


class RevokeData(_Strict):
    revoked: int


class EvaluateEnvelope(_Strict):
    api_version: Literal["1.0"] = API_VERSION
    status: Literal["success", "error"]
    data: EvaluateData | None = None
    error: ErrorInfo | None = None


class RestoreEnvelope(_Strict):
    api_version: Literal["1.0"] = API_VERSION
    status: Literal["success", "error"]
    data: RestoreData | None = None
    error: ErrorInfo | None = None


class RevokeEnvelope(_Strict):
    api_version: Literal["1.0"] = API_VERSION
    status: Literal["success", "error"]
    data: RevokeData | None = None
    error: ErrorInfo | None = None


def _flag(env: dict[str, str], name: str, legacy: str | None = None) -> bool:
    value = env.get(name)
    if value is None and legacy:
        value = env.get(legacy)  # 兼容旧名 DLP_ALLOW_AGENT_RESTORE
    return value == "1"


def build_server(env: dict[str, str] | None = None):
    """创建 MCP server。配置不完整（缺密钥等）时抛异常，进程以非零状态退出（fail closed）。"""
    import anyio.to_thread
    from mcp.server.mcpserver import MCPServer

    from .service import EgressService, ServiceConfig

    env = dict(os.environ if env is None else env)
    tenant_id = env.get("DLP_TENANT_ID")
    actor_id = env.get("DLP_ACTOR_ID")
    if not tenant_id or not actor_id:
        raise RuntimeError("必须配置 DLP_TENANT_ID 与 DLP_ACTOR_ID（可信身份来自服务端配置，而非工具参数）")
    allow_restore = _flag(env, "DLP_ALLOW_RESTORE", legacy="DLP_ALLOW_AGENT_RESTORE")
    service = EgressService(ServiceConfig.from_env(env))
    server = MCPServer("agentnexus-dlp-egress")

    @server.tool(structured_output=True, description=(
        "Check an outbound payload BEFORE it is sent (email, slack, external_api, file_upload, mcp, agent). "
        "Returns decision ALLOW / ANONYMIZE / REVIEW / BLOCK and a sanitized payload. Only send the returned "
        "sanitized payload, and only when data.ready_to_send is true. Pass either `text` or `json_body`."
    ))
    async def egress_evaluate(
        task_id: str,
        channel: str,
        destination_id: str,
        purpose: str,
        text: str | None = None,
        json_body: dict[str, Any] | list[Any] | None = None,
        constraints: list[str] | None = None,
    ) -> EvaluateEnvelope:
        req = EvaluateRequest(
            tenant_id=tenant_id, actor_id=actor_id, task_id=task_id, channel=channel,
            destination_id=destination_id, purpose=purpose, text=text, json_body=json_body,
            constraints=list(constraints or []),
        )
        result = await anyio.to_thread.run_sync(service.evaluate, req)
        return EvaluateEnvelope(status="success", data=EvaluateData(**result.to_dict()))

    @server.tool(structured_output=True, description=(
        "Replace reversible tokens such as [CUSTOMER_001] with their original values, only for tokens created "
        "in the same task, only in the local_ui context. Disabled unless the operator enabled restore."
    ))
    async def egress_restore(task_id: str, text: str, context: str = "local_ui") -> RestoreEnvelope:
        if not allow_restore:
            return RestoreEnvelope(status="error", error=ErrorInfo(
                code="restore_disabled", message="此服务未开启原值恢复（allow_restore: false）"))
        result = await anyio.to_thread.run_sync(service.restore, tenant_id, actor_id, task_id, text, context)
        return RestoreEnvelope(status="success", data=RestoreData(**result.to_dict()))

    @server.tool(structured_output=True, description=(
        "Revoke all reversible token mappings of a task. After revoking, the tokens can never be restored."
    ))
    async def egress_revoke(task_id: str) -> RevokeEnvelope:
        n = await anyio.to_thread.run_sync(service.revoke, tenant_id, task_id)
        return RevokeEnvelope(status="success", data=RevokeData(revoked=n))

    return server


# --config YAML 中的键 → 环境变量名。AgentNexus 的 stdio 配置只有 command/args/cwd，
# 不能单独给子进程设环境变量，所以提供配置文件入口：args = ["--config", "path/to/dlp.yaml"]。
_CONFIG_KEYS = {
    "tenant_id": "DLP_TENANT_ID", "actor_id": "DLP_ACTOR_ID", "vault_path": "DLP_VAULT_PATH",
    "vault_key_file": "DLP_VAULT_KEY_FILE", "destinations_file": "DLP_DESTINATIONS_FILE",
    "master_data_file": "DLP_MASTER_DATA_FILE", "policy_file": "DLP_POLICY_FILE",
    "token_ttl_seconds": "DLP_TOKEN_TTL_SECONDS", "allow_restore": "DLP_ALLOW_RESTORE",
    "allow_agent_restore": "DLP_ALLOW_AGENT_RESTORE",  # 旧名，保留兼容
    "enable_gliner": "DLP_ENABLE_GLINER", "gliner_model": "DLP_GLINER_MODEL",
    "gliner_threshold": "DLP_GLINER_THRESHOLD",
}


def env_from_args(argv: list[str], base: dict[str, str]) -> dict[str, str]:
    """合并 --config 文件与进程环境变量：环境变量优先（便于临时覆盖）。

    配置文件中的相对路径相对于配置文件所在目录解析。密钥本身不允许写进该文件（只能写 vault_key_file）。
    """
    import argparse
    from pathlib import Path

    import yaml

    ap = argparse.ArgumentParser(prog="dlp-egress-mcp")
    ap.add_argument("--config", help="YAML 配置文件路径")
    args = ap.parse_args(argv)
    env: dict[str, str] = {}
    if args.config:
        cfg_path = Path(args.config).resolve()
        raw = yaml.safe_load(cfg_path.read_text(encoding="utf-8")) or {}
        if "vault_key" in raw:
            raise RuntimeError("配置文件中不允许直接写 vault_key，请使用 vault_key_file")
        unknown = set(raw) - set(_CONFIG_KEYS)
        if unknown:
            raise RuntimeError(f"未知配置项：{sorted(unknown)}")
        for k, v in raw.items():
            if isinstance(v, bool):
                v = "1" if v else "0"
            v = str(v)
            if k.endswith(("_file", "_path")) and not Path(v).is_absolute():
                v = str(cfg_path.parent / v)
            env[_CONFIG_KEYS[k]] = v
    env.update(base)
    return env


def main(argv: list[str] | None = None) -> int:
    logging.basicConfig(stream=sys.stderr, level=os.environ.get("DLP_LOG_LEVEL", "INFO"),
                        format="%(asctime)s %(levelname)s %(name)s %(message)s")
    try:
        env = env_from_args(sys.argv[1:] if argv is None else argv, dict(os.environ))
        server = build_server(env)
    except ImportError:
        print(INSTALL_HINT, file=sys.stderr)
        return 1
    except Exception as exc:  # noqa: BLE001 —— 配置错误直接退出，不以不安全配置运行
        print(f"dlp-egress-mcp 启动失败：{exc}", file=sys.stderr)
        return 2
    server.run(transport="stdio")
    return 0


if __name__ == "__main__":
    sys.exit(main())
