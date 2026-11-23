# AI Desktop T-78：记忆持久化表（夹具 → SQLite）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23（America/Vancouver） |
| 来源 | **助手建议**（授权开方案卡；**待用户/协调员确认后**才改 `store.rs`） |
| 状态 | **待用户确认** |
| 影响范围 | 特性 ai-desktop / MOCK M3；`crates/runtime/src/store.rs` DDL；`desktop_api` memories 路由；RT-ADMEM / E2E-ADMEM/ADMEMW |
| 相关设计 | [ai-desktop/DESIGN.md](../ai-desktop/DESIGN.md) §5 memories；[MOCK_REGISTRY](../ai-desktop/MOCK_REGISTRY.md) M3；T-71/T-72 证据 |

**一句话结论（待批）**：把 `/api/desktop/v1/memories*` 从进程内 `OnceLock` 夹具迁到 SQLite 表 `desktop_memories`；**HTTP 契约与 ADMEM/ADMEMW 断言不变**；真实模式以 Store 为权威；CI 保留 `fixture/reset` 仅作种子复位，不另起权威源。

## 背景

T-71/T-72 已冻结列表/纠错/撤回合同，实现为 `desktop_api` 内 `Mutex<Vec>`。MOCK_REGISTRY M3 写明「持久化记忆表仍待」。本卡按授权做 Store 迁移。

## 问题

1. 进程重启后记忆丢失，无法证明「真实模式不以 fixture 为权威」。
2. 改 `store.rs` / DDL 属 AGENTS.md「先停下来问人」——须方案批准后再实现。

## 决策依据

1. 用户授权 T-78 范围（#234 后）；硬门槛：先方案后实现。
2. DESIGN：`GET/PATCH/DELETE /memories*`；首版夹具、持久化另卡（本卡即该另卡）。
3. 既有 RT-ADMEM-01～06、E2E-ADMEM/ADMEMW：**不得弱化断言**；种子文案（`mem-1`/`mem-2`）保持。
4. 同路径先例：`desktop_preferences` / `desktop_evidence` 用 `CREATE TABLE IF NOT EXISTS` 兼容建表，无独立迁移工具。

## 定了什么（待确认）

### 1. 表结构

```sql
CREATE TABLE IF NOT EXISTS desktop_memories (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL DEFAULT 'local',
    title TEXT NOT NULL,
    source TEXT NOT NULL,
    body TEXT NOT NULL,
    expires TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_desktop_memories_user
    ON desktop_memories(user_id, revoked);
```

| 列 | 对应现 API 字段 | 说明 |
|---|---|---|
| `id` | `id` | 稳定 ID；种子 `mem-1`/`mem-2` |
| `user_id` | （响应不暴露） | 与 preferences 同占位 `local`；本卡不做多租户/IdP |
| `title` / `source` / `body` / `expires` | 同名 | 字符串语义不变 |
| `revoked` | `revoked`（列表过滤；GET 已撤回 → 410） | `0/1` |
| `updated_at` | （可不序列化） | ISO 或既有 store 时间格式 |

**不加**：version/乐观锁（现 PATCH 无 `expected_version`）；抽取元数据；RelOS 外键。

### 2. 迁移方式

- 在 `Store` 打开时的 `SCHEMA`（或等价 `execute_batch`）追加 `CREATE TABLE IF NOT EXISTS` + 索引。
- **无**独立 SQL 迁移文件、无破坏性 `DROP`、无改既有表列。
- 旧库：首次打开自动建表；空表时由 `ensure_desktop_memory_seed`（名待定）插入与今日 `seed_memories()` **相同**两条种子（`INSERT OR IGNORE`）。
- 已有行：不覆盖用户纠错/撤回结果。

### 3. API 契约（保持）

| 方法 | 行为 | 状态码 |
|---|---|---|
| `GET /memories` | 非 revoked 列表 | 200 `{ items }` |
| `GET /memories/{id}` | 详情；已撤回 | 200 / 410 `revoked` / 404 |
| `PATCH /memories/{id}` | 可选 `title`/`body`/`expires`；已撤回不可改 | 200 / 410 / 404 |
| `DELETE /memories/{id}` | 软删 `revoked=1`；再删幂等 | 200 `{ ok, id, revoked }`；未知 id 404 |
| `POST /memories/fixture/reset` | **仅测试**：清空该 user 记忆行并重播种子 | 200 `{ ok, reset }` |

- 处理器从 `State`/`Store` 读写；**删除**进程内 `OnceLock` 夹具。
- 生产/真实模式：**不以**内存夹具为权威；`fixture/reset` 可保留在路由上供 harness（与 evidence/voice reset 同模式），但文档标明测试专用。
- **前端** `api.ts` / SettingsPanel：路径与 DTO 不变；E2E 仍可调 `fixture/reset`。

### 4. Store API（建议表面）

- `ensure_desktop_memory_seed(user_id)`
- `list_desktop_memories(user_id) -> Vec`（过滤 revoked）
- `get_desktop_memory(user_id, id) -> Option`（含 revoked 行，供 410）
- `patch_desktop_memory(...)`
- `revoke_desktop_memory(...)`（幂等）
- `reset_desktop_memories(user_id)`（测试）

**不改** `orchestrator.rs`、任务状态机、公开受理 API。

### 5. 用例与证据

- **保留**：RT-ADMEM-01～06、E2E-ADMEM、E2E-ADMEMW（断言目标不变）。
- **补强（实现卡）**：可选 RT-ADMEM-07「同库重启后 PATCH 结果仍在」（证明 Store 权威）；证据 `docs/testing/evidence/ai-desktop-t78-memory-persist.md`。
- 更新 MOCK_REGISTRY M3：记忆 → **Store 持久**；夹具 reset 仅测试种子。

### 6. 回滚

1. **代码**：revert 实现 PR；表可留空（`IF NOT EXISTS` 无害）或后续另卡清理。
2. **数据**：无强制回填夹具；测试靠 `fixture/reset`。
3. **无**跨版本数据迁移脚本需回滚。

## 没选什么

| 做法 | 为什么没选 |
|---|---|
| payload_json 单行 blob（仿 preferences） | 列表过滤 revoked、按 id PATCH 更适合列式；与 T-71 字段一一对应 |
| 本卡上乐观锁 / IdP 多用户 | 契约无 version；非目标含 IdP |
| 删掉 `fixture/reset` | 破坏 ADMEMW/RT 串行复位；改为 Store 实现即可 |
| 记忆抽取 / RelOS / 阅读锚点 | 明确非本卡 |
| 改 orchestrator | 禁止 |

## 实现卡出口（批准后）

1. 冻结/确认用例（含可选 RT-ADMEM-07）→ Red（若新断言）→ Store + API → Green。
2. `cargo test -p an-server --test desktop_mem`；Playwright ADMEM/ADMEMW + 全桌面回归。
3. 更新 MOCK_REGISTRY / PLAN / DESIGN 一句；开实现 PR；**不自开下一卡**。

## 出问题先看

- 列表是否仍过滤 `revoked`；GET 已撤回是否仍 410（非 404）。
- `fixture/reset` 是否写库而非内存。
- 种子 id/文案是否仍匹配 E2E「质量报告偏好摘要格式」。

## 变更记录

| 日期 | 变化 | 关联 |
|---|---|---|
| 2026-11-23 | 新建方案（待确认）；未改 store | T-78 授权开方案 |
