# 证据：conversation-memory S5-route 路由路径会话资料线索

- **日期**：2026-11-24（America/Vancouver）
- **授权**：用户 2026-11-24 沿用 S4 拍板（极简清单 / 默认开 / 独立开关 / 先合入后跑 E0 对比）；决策 `docs/decisions/2026-11-24-convmem-s5-route-materials.md`
- **范围**：`conversation.rs`（Delegate 分支 ctx 追加线索段 + `session_materials_route`）、`config.rs`、`lib.rs`；**router.rs 零改动**（线索并入既有 ctx 入口）；未改 orchestrator
- **契约**：RT-RMAT-01～03（新增）；既有全链保留

## Case

| Case | 结果 |
|---|---|
| RT-RMAT-01 线索段进路由上下文 | Green（user 含「会话资料线索」段与任务 id） |
| RT-RMAT-02 独立开关 | Green（关 = ctx 原样） |
| RT-RMAT-03 无关发现不进 | Green（相关性门） |

## Red → Green

```text
# Red（ctx 追加未实现）
cargo test -p an-server --test route_materials --locked → 2 failed / 1 passed
# Green
→ 3 passed
```

## 命令与结果

```text
# America/Vancouver 2026-11-24
cargo fmt / clippy -D warnings / workspace / web 四件 → 通过（本机既有失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
```

## E0 对比（合入后执行项）

同 S4：标准集与评测直驱均无资料绑定 → 提示词逐字节不变，理论零变化；材料化评测集另卡。

## 非本卡

执行路径最小材料（S 系列下一张）；材料化评测集。
