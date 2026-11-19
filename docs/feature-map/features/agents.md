# agents：助手（agent）的发现、接入、状态与排查

覆盖 features.json 的 agent-links、acp-discovery、agent-catalog、agent-config、agent-ui、agent-chat、agent-diagnose。

## Sub-features

| 子功能 | 说明 |
|---|---|
| agent-links | 左下角每个 agent 一行：绿=已连通、灰=已配置未连通、红=不可用；原因在悬停提示/行内 |
| acp-discovery | 启动时自动发现预设命令（opencode、hermes、pi-acp、claude-agent-acp、codex-acp、cursor-agent、zcode-acp-server），注册为 ACP 卡片；未安装不注册不报错；用户同 id 配置优先；通用名 `agent` 不认作 Cursor |
| agent-catalog | `GET /api/agents/catalog`、`POST /api/agents/probe`：目录、本机状态、握手检测；分类 ok / not_installed / needs_login / missing_dependency / handshake_failed / timeout；只接受 id，带 command 的请求被拒 |
| agent-config | `POST /api/agents/proposals`（只校验不写入）→ `POST /api/agents/proposals/{id}/apply`（确认后写托管文件、热更新）；可连续撤销；`config.toml` 优先；`GET /api/agents/config` |
| agent-ui | 右栏“助手”页签：列表、状态、来源、用途、安装引导、重新检测、确认卡→结果卡、撤销、添加自定义助手 |
| agent-chat | 对话里说“把 Cursor 接进来”（需 `[conversation] agent_management`）→ 前台判 `manage_agents` → 确认卡/选择卡/引导卡/发现卡 |
| agent-diagnose | “排查”：握手检测 + 只读本机体检 + 规则结论；诊断包；请其他助手帮忙（每次预览、逐次同意；经 egress 脱敏） |

## How to get to it (user POV)

- 看状态：左栏底部连接状态区（“连接详情 ▾”看后端地址/模型）。
- 管理：右栏“**助手**”页签 → “助手列表”；首次打开页面对话区顶部有“发现的助手”卡（“知道了”关闭）。
- 接入：行内“接入” → “确认修改”卡 → “确认” → “修改结果”卡；“撤销上一次修改”。
- 没装：“看安装步骤” → 复制命令安装 → “我装好了，重新检测”。
- 对话接入：发“把 Claude Code 接进来”。
- 出问题：行内“排查”。

## Driving it with Playwright / HTTP

```ts
await page.getByRole('tab', { name: '助手' }).click();
const list = page.getByRole('region', { name: '助手列表' });
await list.locator('…<含 claude 的行>').getByRole('button', { name: '接入' }).click();
const card = page.getByRole('region', { name: '确认修改' });
await card.getByRole('button', { name: '确认', exact: true }).click();
await expect(page.getByRole('region', { name: '修改结果' })).toContainText('已连通');
await page.getByRole('button', { name: '撤销上一次修改' }).click();
```

E2E 环境里 `harness.mjs` 把 `claude-agent-acp` 做成指向 `an-mock-acp` 的包装脚本放进 PATH，并提供一个启动即失败的 `broken-agent`（错误输出含“客户 ACME”）和模拟 DLP 服务，用来验证第 3、4 层排查。对话接入的受控触发词：“把 Claude Code 接进来验证”“接个写代码的助手验证”；让路由到 coding：“改代码验证：…”。

HTTP：

```bash
curl -s localhost:8787/api/agents | jq .
curl -s localhost:8787/api/agents/catalog | jq .
curl -s -XPOST localhost:8787/api/agents/probe -H 'content-type: application/json' -d '{"id":"claude"}'
curl -s -XPOST localhost:8787/api/agents/diagnose -H 'content-type: application/json' -d '{"id":"zcode"}'
```

确认提议：`POST /api/agents/proposals/{id}/apply`，body `{"token": "<提议返回的凭证>"}`；提议请求体见 `web/src/api.ts` 的 `ChangeRequest`（`{kind:"add_preset",id}` 或 `{kind:"add_custom",agent}`）。

## Verify（判据）

- 内置 agent 恒为绿；命令不存在为红；能启动但握手失败为灰；`/api/health` 每个 agent 带 `link.status`。
- 提议阶段**不写入**；确认后注册表出现该 agent；撤销后恢复原状。
- 停用最后一个写代码助手后，要求改代码的对话得到大白话回复“现在没有能写代码的助手，这件事还没有开始做”，且**没有任务被创建/派给别人**。
- 探测超时后被探测的子进程已结束；检测成功后连接状态变绿。
- 诊断（第 1、2 层）不外发；第 4 层发送前必有完整预览，且只发 DLP `ready_to_send` 的脱敏文本。

## Gotchas

- 写在 `config.toml` 的助手在界面里**没有“停用”按钮**，这是预期。
- 绿色表示“握手成功或 10 分钟内有成功调用”，只是可调用，不等于模型凭证/额度有效。
- 真实 claude / codex / cursor / zcode 的登录与依赖错误文字未在沙箱验证，分类特征可能需按实测补充。
- 助手使用**它们自己的**模型配置，不会自动走本地 llama.cpp。
- 默认对 agent 的权限请求自动允许（逐次记录）；用 `permission = "read_only"` 收紧。
- 执行出错直接报错，**不会自动换别的 agent 重做**。
- `agent-catalog`/`agent-config` 的 HTTP 接口先于界面存在；浏览器只覆盖“助手”页签的主路径。

## Backed by

源码：`crates/agents/src/acp.rs`（高风险：子进程）、`crates/runtime/src/link.rs`、`web/src/components/AgentsPanel.tsx`、`AgentCards.tsx`、`web/src/App.tsx`（`HealthBox`）
用例：E2E-AGFIRST、AGLIST、AGTOML、AGGUIDE、AGCONNECT、AGUNDO、AGDISABLE、AGCHAT、AGCHOICE、AGDIAG、AGHELP（`web/e2e/agents.spec.ts`）；RT-ACPD-01～05、RT-AGCAT-01～09、RT-AGCFG-01～14、RT-AGCHAT-01～09、RT-AGDIAG-01～12
验证状态：用模拟 ACP agent 的浏览器与 HTTP 路径已覆盖；真实 agent 的握手/登录未在沙箱验证。
