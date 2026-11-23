# 证据：conversation-memory 5.2a 个人记忆生命周期与隔离

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户 2026-11-23 四项拍板（范围切分 / POST 端点 + kind 补列 / 夹具头隔离口径 / 红线归 5.2b）；决策 `docs/decisions/2026-11-23-convmem-s2a-lifecycle-and-isolation.md`
- **范围**：`store.rs`（kind 补列迁移 + `create_desktop_memory` + `list_unrevoked_desktop_memories`）、`desktop_api.rs`（POST + `principal_or_local` 作用域）、`session_materials.rs`（记忆段 + 过期过滤 + 逐条预算）、`conversation.rs` / `lib.rs`（说话人传播）；未改 orchestrator；未改 evidence ACL 的 `alice` 契约
- **契约**：RT-ADMEM-01～07 全链保留；新增 RT-UMEM-01～05；E2E 待第 1 线保存入口后补录

## Case

| Case | 结果 |
|---|---|
| RT-UMEM-01 保存可查 | Green（201 + mem- id + kind + 来源「用户保存」；列表/详情可见） |
| RT-UMEM-02 生命周期 | Green（纠正 200、撤回 200、详情 410、列表排除；缺省有效期=长期） |
| RT-UMEM-03 双用户隔离 | Green（乙列表不含 / 详情 404 / PATCH 404 / DELETE 404；甲本人 200） |
| RT-UMEM-04 过期不入注入 | Green（过期条 id 与正文均不在资料块；未过期条带来源行） |
| RT-UMEM-05 按说话人作用域 | Green（甲续问含甲的记忆；乙续问不含） |
| RT-SMAT-01～05 回归 | Green（含逐条截断修订后） |

## 实现要点

- `kind TEXT NOT NULL DEFAULT 'fact'`：SCHEMA 加列 + `Store::open` 内 `pragma_table_info` + `ALTER TABLE` 迁移（旧库自动补列）。
- `principal_or_local`：夹具头非空用之，否则 `local`；evidence 的 `principal_from`（无头 `alice`）不动。
- 注入块「任务回执 + 个人记忆」两段；预算按条均分、逐条 `clip_middle`，来源行永不截断。

## 命令与结果

```text
# America/Vancouver 2026-11-23
CARGO_TARGET_DIR=target cargo test -p an-server --test personal_memory --locked   → 5 passed
CARGO_TARGET_DIR=target cargo test -p an-server --test session_materials --locked → 5 passed（回归）
CARGO_TARGET_DIR=target cargo fmt --all -- --check / clippy --locked --all-targets -- -D warnings → 通过
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast            → 全绿（既有本机失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
cd web && npm run typecheck && npm test && npm run test:map && npm run build      → 通过
```

## 非本卡

预览与下一会话生效闸门（5.2b）；保存入口 UI（第 1 线）；组织范围（5.4）。
