# #419 本机 Claude Code 接入复验（macOS）

| 项 | 值 |
|---|---|
| 日期 | 2026-11-26 20:41–20:53 America/Vancouver |
| 测口 | `http://127.0.0.1:8787` |
| 部署 commit | `cfd9bd9`（跑测时）；证据 PR 基线 `384e070` |
| OS | macOS 26.6.2 arm64 |
| Claude Code | `2.1.329`（`npm i -g @anthropic-ai/claude-code`，`~/.npm-global/bin/claude`） |
| claude-agent-acp | `0.112.0`（`@agentclientprotocol/claude-agent-acp`，同前缀 PATH） |
| Node | v22.22.0 |
| 配置 | `[acp] auto_detect=true`；`[agents.assign] coding="claude"`；服务进程 PATH 含 `~/.npm-global/bin` |

原始 JSON：同目录 `line6-419-*.json` / `line6-419-env.txt`。

## 步骤与结果

### 1）已安装：发现 / 检测 / 体检一致 → **PASS**

| 接口 | 结果 |
|---|---|
| `GET /api/agents/catalog`（claude） | `status=registered`，`source=auto`，`link_status=connected` |
| `POST /api/agents/probe` | `kind=ok`，「已连通：握手成功」 |
| `POST /api/agents/diagnose` | probe=`ok`；三项检查均 `ok`（服务 PATH / login shell / Node）；**无**「体检 ok 但检测 not_installed」矛盾 |

诊断包摘要（脱敏后）：能找到 `~/.npm-global/bin/claude-agent-acp`；检测结果 ok。

### 2）委派小任务由 Claude Code 执行 → **PASS**

- 对话委派编码任务：在 `workspace/code` 的 README.md 末尾追加 `#419-probe`，并 `git diff`。
- 任务 `task-bfa712be07ce42648888a5515ac1ab9f`：`role=coding`，`assignment.agent_id=claude`，`kind=acp`。
- 状态 `COMPLETED`，`result.status=succeeded`；evidence `executed_by: Claude Code（acp，本机）`；diff 确认追加成功。

### 3）隐藏 `claude-agent-acp` 后再检测 → **PASS**

- 临时 `mv` 掉 PATH 上的 `claude-agent-acp` 并重启服务。
- catalog：`status=not_installed`，install 提示含 `npm i -g @agentclientprotocol/claude-agent-acp`。
- probe：`kind=not_installed`，`next_step` 给出安装/登录指引。
- diagnose：probe=`not_installed`；checks 两处 command 为 `problem`；findings 规则 `not_installed` 带可操作 steps。
- **未出现**「体检 ok / 检测 not_installed」分裂。
- 测完已恢复二进制并重启 8787。

## 结论

相对 #325 原缺陷，本机 macOS 上检测与体检一致；已安装可实跑任务；未安装明确 `not_installed` 并给安装指引。**建议关闭 #419。**

## 非目标 / 说明

- 未补 Windows/Linux。
- 跑测时 coding 指派临时改为 `claude`（本地 `.tmp-config.toml`），非仓库默认配置。
