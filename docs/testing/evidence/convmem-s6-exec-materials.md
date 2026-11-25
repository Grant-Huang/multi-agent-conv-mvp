# 证据：conversation-memory S6-exec 执行路径会话资料完整块

- **日期**：2026-11-24（America/Vancouver）
- **授权**：用户 2026-11-24 四项拍板（完整块 / 独立开关+512 / 仅 general / 随任务持久）；决策 `docs/decisions/2026-11-24-convmem-s6-exec-materials.md`
- **范围**：`conversation.rs`（delegate 任务上下文追加 + `session_materials_exec*` + Role 过滤）、`config.rs`、`lib.rs`；未改 GeneralAgent/AgentAdapter trait、未改 orchestrator、未改 store.rs
- **契约**：RT-EXEC-01～04（新增）；既有全链保留

## Case

| Case | 结果 |
|---|---|
| RT-EXEC-01 完整块进执行背景 | Green（标记 + 摘要全文 + 来源 id） |
| RT-EXEC-02 512 上限预裁 | Green（来源行保留、长度有界） |
| RT-EXEC-03 开关关 | Green（需同时关 route 线索段以隔离验证——两开关独立） |
| RT-EXEC-04 随任务持久 | Green（`tasks.context` 列含资料段，经 `get_task_context` 验证） |

## Red → Green

```text
# Red（任务上下文追加未实现）
cargo test -p an-server --test exec_materials --locked → 4 failed
# Green
→ 4 passed
```

## 命令与结果

```text
# America/Vancouver 2026-11-24
conversation-memory 七套件（exec/UFND/SMAT/JMAT/UMEM+USTY/RMAT/desktop_mem）→ 37/37 全绿
cargo fmt / clippy -D warnings / workspace / web 四件 → 通过（本机既有失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
```

## 非本卡

材料化评测集（需评测口径确认）；E0 对比执行（#290/#326 合并后）；5.4（第 4 线）。
