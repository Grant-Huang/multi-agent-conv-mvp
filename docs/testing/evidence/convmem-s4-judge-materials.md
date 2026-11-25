# 证据：conversation-memory S4 判断路径会话资料清单

- **日期**：2026-11-24（America/Vancouver）
- **授权**：用户 2026-11-24 四项拍板（极简清单 / 先合入后跑 E0 对比 / 默认开 / 仅判断路径）；决策 `docs/decisions/2026-11-24-convmem-s4-judge-materials.md`
- **范围**：`session_materials.rs`（`render_judge_digest`）、`conversation.rs`（收集上移每轮一次 + decide_turn 注入 + `session_materials_judge`）、`config.rs`、`lib.rs`、`bin/eval.rs`（评测直驱传 None）；未改 turn_decision schema、未改 store.rs、未改 orchestrator
- **契约**：RT-JMAT-01～05（新增）；既有全链保留

## Case

| Case | 结果 |
|---|---|
| RT-JMAT-01 回执出窗进判断清单 | Green |
| RT-JMAT-02 清单三行齐全（回执/记忆/发现） | Green |
| RT-JMAT-03 发现相关性门 | Green（无关提问不注入发现行；回执/记忆不受门限为既定语义） |
| RT-JMAT-04 超时降级 | Green（判断照常） |
| RT-JMAT-05 行截断 ≤40 字 | Green |

## Red → Green

```text
# Red（判断注入未实现）
cargo test -p an-server --test judge_materials --locked → 3 failed / 2 passed
# Green
→ 5 passed
```

## 命令与结果

```text
# America/Vancouver 2026-11-24
cargo fmt / clippy -D warnings / workspace / web 四件 → 通过（本机既有失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
```

## E0 对比（合入后执行项）

标准 E0 数据集无资料绑定 → 判断提示词逐字节不变（评测直驱传 None + 会话路径 digest 恒空），理论零变化；材料化评测集另卡，结果回填 FT 记录。

## 非本卡

路由/执行路径（另卡）；材料化评测集；5.4（已移交第 4 线）。
