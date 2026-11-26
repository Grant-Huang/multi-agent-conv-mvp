# 本地可部署性证据（W1 / #424）— 进行中

- **日期**：2026-11-26（America/Vancouver）
- **线**：第 6 线
- **基线**：`origin/master` @ `300f9b7`；分支 `cursor/line6-w1-deployability`
- **模型**：本地 llama `http://127.0.0.1:8200/v1`（Qwen3.8-35B-A3B-Q4_K_M）
- **实验室目录**：`/tmp/line6-deploy-lab`（不入库）

## D2 配置完整性（PR #430）

对照 `AppConfig`，`config.example.toml` 已补 `[desktop]` / `[mom]` / `[introspection]` 与 `session_materials` 注释；`README.md` 快速开始补充 AI 桌面 / MOM / 记忆 / 内省。最小配置可启动到 `server.started`。

## D1 冷启动（本机 macOS，按 README + manual）

| 步骤 | 结果 | 耗时（约） |
|---|---|---|
| 复制 `config.example.toml` → 改 `bind`/`db_path`/`[llm].base_url` | 可行；样例默认 8098，本机模型在 8200，需改 | <1 min |
| `cd web && npm install && npm run build` | 通过（已有 node_modules 时 build ~0.3s） | 视环境 1～5 min |
| `cargo build --release --bin agentnexus` | 通过 | ~1–2 min（热）/ 更长（冷） |
| 启动并打开健康检查 | `server.started`；`[acp] auto_detect=true` 时本机 ACP 探测较吵，建议冷启动文档注明可先 `auto_detect=false` | <5 s |
| 建会话 + `POST .../messages` 字段 | 请求体字段为 **`text`**（不是 `content`）；错字段直接 422 | — |
| 首轮真实模型问答 | HTTP 200；用户/助手各 1 条落库（例：SMT-03 提问，约 47s） | ~1 min |

**卡点（文档）**：升级前 README/manual 偏早期聊天 MVP；本 PR 已补 README 与 [09-backup-and-upgrade.md](../../manual/09-backup-and-upgrade.md)。API 字段名未在快速开始写明——记入手册后续可补「发消息」示例。

**未做**：全新用户账号隔离环境（本机已有 Rust/Node/模型）；严格「只看文档、零先验」仍欠一次干净机复验。

## D5 升级（`v1.0.0` → 当前二进制）

1. `v1.0.0`（tag `0d0133c`）在 `127.0.0.1:18788` 启动，空库。
2. 建会话 `conv-feab48656b8b483d863280dbb2780137`，提问「D5种子问题：记住 SMT-03。」→ 助手回复落库（2 条消息）。
3. 停旧进程；同 `db_path` 启动当前 release。
4. **结果**：健康检查 OK；会话 1、消息 2 均在；升级日志无 panic / SQL 错误。
5. 升级后经桌面 API 写入记忆 `mem-ba6b61e8a7e442499090d50079dc0ad2`（title=`D5种子`）。

W2 数据集未就绪，本轮用接口手工造数（符合 #424 正文）。

## D6 回滚

- 用 **`v1.0.0` 二进制**直接打开「已由新版写过」的库：**能启动**，旧会话列表可读。
- **不符合** LOCAL_E2E「明确告知哪些新数据不可用」：旧版无提示，个人记忆等新面在旧二进制上不可管理且无告警。
- 已开 `[待归属]` Issue 给第 2 线（见下）。**实用建议**（已写入手册）：回滚优先用升级前的库备份。

## D7 备份与恢复

1. 停服务 → `PRAGMA wal_checkpoint(FULL)` → 拷贝 `agentnexus.db` + 配置。
2. 删除 `data/*` 后恢复备份，用当前二进制启动。
3. **结果**：会话 1、消息 2、记忆含 `D5种子` 均在（列表另见夹具/历史 `mem-1`，与本种子并存）。

手册已补：`docs/manual/09-backup-and-upgrade.md`。

## 清单状态

| 项 | 状态 |
|---|---|
| D1 | 部分通过（本机路径记录；欠干净机） |
| D2 | 通过（等审合） |
| D3/D4 | 未做（可与 #419 交叉） |
| D5 | 通过（手工种子） |
| D6 | **偏离**：能启无「新数据不可用」提示 → 待归属 |
| D7 | 通过（手工种子；W2 verify 脚本未就绪） |

**不关闭 #424**，直至上表与验收齐套。
