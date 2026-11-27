# W1-D8 清理与卸载 — 2026-11-27

| 项 | 值 |
|---|---|
| Issue | #424 |
| 日期 | 2026-11-27（America/Vancouver） |
| 基线 | `origin/master` @ 开工时最新（证据 PR 合入时写明） |
| 实验室 | `/tmp/line6-d8-lab`，`bind=127.0.0.1:28787`，`db_path=data/d8.db` |
| 二进制 | `target/release/agentnexus` |
| 模型 | 本机 `:8200` Qwen3.8-35B（仅用于写入一条会话） |
| 未动 | `8787` 上的 #426 8h soak |

## 步骤与结果

1. 启动实验室实例 → `/api/health` **200**。
2. `POST /api/conversations` + 一问「好」→ 落库；出现 `data/d8.db`（含 wal/shm）与 **`data/d8.mom.db`**（即使未显式开 MOM UI，旁路库仍可能创建）。
3. `kill -TERM <pid>` → 约 2s 内端口 `28787` **无监听**；`ps` 无 `line6-d8-lab` 相关 `agentnexus`。
4. 删除主库与 wal/shm、清空 `logs/`、`workspace/` 后，**仍残留** `d8.mom.db`。
5. 补删 `*.mom.db` 后 `data/` 为空。

## 判定

| 标准 | 结果 |
|---|---|
| 停服务后无本实例后台进程 | **通过** |
| 文档写清数据位置 | 原有「数据在哪里」表已列主库/MOM/workspace/logs；本轮补 **D8 清理步骤**（含 MOM 库） |
| 删数据可干净 | **通过**（须显式包含 `*.mom.db`） |

## 文档改动

- `docs/manual/09-backup-and-upgrade.md`：更名覆盖 D8；新增「清理与卸载」。
- `docs/manual/README.md`：索引改为含卸载。

## 非目标 / 仍欠（#424 不关）

- D1 **全新 OS** 账号口径仍欠。
- D9 多平台未做。
- 未卸载本机 Rust / Node / llama.cpp（不在 D8 范围）。
