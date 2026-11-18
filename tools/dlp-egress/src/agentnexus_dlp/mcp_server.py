"""stdio MCP 服务：把外发脱敏服务暴露给 AgentNexus（或任何 MCP 客户端）。

身份边界：
- tenant_id / actor_id 只从服务端环境变量（DLP_TENANT_ID / DLP_ACTOR_ID）读取，
  工具参数里不提供这两个字段，Agent 无法自报身份（设计文档第 3 节第 1 步）。
- egress_restore 默认关闭（DLP_ALLOW_AGENT_RESTORE=0）：原值恢复应在本地可信 UI 或
  任务后处理步骤中进行，而不是交还给可能继续外发的 Agent。

stdout 留给 MCP 协议，日志（含审计日志）只写 stderr。
"""
from __future__ import annotations

import json
import logging
import os
import sys
from typing import Any

from .models import EvaluateRequest

INSTALL_HINT = "未安装 MCP 支持，请安装可选依赖：pip install 'agentnexus-dlp[mcp]'"


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
    allow_restore = env.get("DLP_ALLOW_AGENT_RESTORE", "0") == "1"
    service = EgressService(ServiceConfig.from_env(env))
    server = MCPServer("agentnexus-dlp-egress")

    def dump(obj: Any) -> str:
        return json.dumps(obj, ensure_ascii=False)

    @server.tool(structured_output=False, description=(
        "Check an outbound payload BEFORE it is sent to email, Slack, an external API, a file upload, "
        "another MCP server or a cloud LLM. Returns decision ALLOW / ANONYMIZE / REVIEW / BLOCK and a "
        "sanitized payload. Only send the returned sanitized payload, and only when ready_to_send is true. "
        "REVIEW and BLOCK mean: do not send. Pass either `text` or `json_body`, not both."
    ))
    async def egress_evaluate(
        task_id: str,
        channel: str,
        destination_id: str,
        purpose: str,
        text: str | None = None,
        json_body: dict[str, Any] | list[Any] | None = None,
        constraints: list[str] | None = None,
    ) -> str:
        req = EvaluateRequest(
            tenant_id=tenant_id, actor_id=actor_id, task_id=task_id, channel=channel,
            destination_id=destination_id, purpose=purpose, text=text, json_body=json_body,
            constraints=list(constraints or []),
        )
        result = await anyio.to_thread.run_sync(service.evaluate, req)
        return dump(result.to_dict())

    @server.tool(structured_output=False, description=(
        "Replace reversible tokens such as [CUSTOMER_001] in a response with their original values, "
        "only for tokens created in the same task. Disabled unless the operator enabled agent restore."
    ))
    async def egress_restore(task_id: str, text: str, context: str = "task_postprocess") -> str:
        if not allow_restore:
            return dump({"status": "error", "code": "restore_disabled",
                         "message": "此服务未允许 Agent 恢复原值；请在本地可信界面中恢复"})
        result = await anyio.to_thread.run_sync(service.restore, tenant_id, actor_id, task_id, text, context)
        return dump({"status": "success", **result.to_dict()})

    @server.tool(structured_output=False, description=(
        "Revoke all reversible token mappings of a task. After revoking, the tokens can never be restored."
    ))
    async def egress_revoke(task_id: str) -> str:
        n = await anyio.to_thread.run_sync(service.revoke, tenant_id, task_id)
        return dump({"status": "success", "revoked": n})

    return server


# --config YAML 中的键 → 环境变量名。AgentNexus 的 stdio 配置只有 command/args/cwd，
# 不能单独给子进程设环境变量，所以提供配置文件入口：args = ["--config", "path/to/dlp.yaml"]。
_CONFIG_KEYS = {
    "tenant_id": "DLP_TENANT_ID", "actor_id": "DLP_ACTOR_ID", "vault_path": "DLP_VAULT_PATH",
    "vault_key_file": "DLP_VAULT_KEY_FILE", "destinations_file": "DLP_DESTINATIONS_FILE",
    "master_data_file": "DLP_MASTER_DATA_FILE", "policy_file": "DLP_POLICY_FILE",
    "token_ttl_seconds": "DLP_TOKEN_TTL_SECONDS", "allow_agent_restore": "DLP_ALLOW_AGENT_RESTORE",
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
