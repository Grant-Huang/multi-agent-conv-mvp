# MCP 开发计划

本次已获授权按设计实施第一阶段，后续阶段保留明确验收边界。

- [x] 开发前 fetch 并快进同步 origin/master `929301a`，确认无用户未提交改动。
- [x] 审查现有 General、ToolRegistry、Router、Policy、Artifact 与 MOM 设计，形成 [DESIGN.md](DESIGN.md)。
- [x] 先编写 MCP 协议与安全边界回归测试，确认缺少实现时编译失败。
- [x] 实现统一能力卡片、官方 SDK 客户端、离线 Schema 校验和统一结果。
- [x] 接入配置、异步启动、General 网关与能力目录 API；服务与评测 CLI 共用异步发现。
- [x] 添加配置/开发说明，完成测试、Clippy、前端构建和自查。
- [ ] 第二阶段：二进制产物存储、下载与预览；真实搜索、Mermaid、PPTX 提供方集成和验收。
- [ ] 第三阶段：按需检索、热刷新与 Reviewer 只读工具。
- [ ] 第四阶段：外部写入审批、幂等与受理恢复。

公共 MCP 服务和真实模型不作为第一阶段离线验收依赖；没有实测就不宣称可用。

## 第一阶段验证记录

- `CARGO_TARGET_DIR=target cargo test --locked`：210 passed，0 failed，2 ignored（仓库默认真实模型槽位/mDNS 检查）。
- `cargo clippy --locked --all-targets -- -D warnings`：`Finished dev profile`，退出码 0，无警告。将既有 LLM SSE 首 token 判断改为等价简洁布尔表达式，未关闭检查。
- `cargo fmt --all -- --check`：通过。
- `cd web && npm test`：`Test Files 4 passed (4)`、`Tests 17 passed (17)`。
- `cd web && npm run build`：`tsc --noEmit && vite build`，`✓ 278 modules transformed`，构建通过。
- 实际 agentnexus 二进制启动：`GET /api/capabilities` 返回 fixture.echo 及正确 Schema，静态前端返回成功。
- 仓库真实 Python websearch-mcp：完成 initialize、tools/list、Rust Schema 编译及目录 API 验证；没有发送外部搜索查询。
- 新测试覆盖真实 stdio/HTTP JSON-RPC、分页、允许清单、参数错误不外发、工具错误、输出 Schema、取消与 Linux 子进程回收、超时连接丢弃、禁止重定向、HTTP 会话过期不重放、General 选工具与读取结果、Runtime/API 接线。General 测试夹具由最初错误的普通 JSON 响应改为仓库现有 SSE 协议，保留全部断言。

限制：目录是固定启动快照；同一服务器串行；失效连接需重启。调用级状态未知没有持久化恢复，远端取消不保证停止；stdio 不是 OS 沙箱。二进制产物、真实 Mermaid/PPTX 提供方与真实 LLM 效果未验收。
