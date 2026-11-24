# 证据：conversation-memory 5.3 内省发现按需进入会话

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户 2026-11-23 四项拍板（确定性 bigram 匹配 / 待核实标注注入 / 过闸门即可候选 / 2 条独立计）；决策 `docs/decisions/2026-11-23-convmem-s3-findings-in-session.md`
- **范围**：`session_materials.rs`（发现来源 + dice 相关性门 + 「内省发现」段）、`conversation.rs` / `config.rs` / `lib.rs`（传当前提问 + `session_materials_findings_max` 默认 2）、`store.rs` 零改动（只读复用 `introspection_list_findings`）；未改 orchestrator、未改 INT 写路径
- **契约**：RT-SMAT-01～05、RT-UMEM-01～05 全链保留；新增 RT-UFND-01～05

## Case

| Case | 结果 |
|---|---|
| RT-UFND-01 相关性门 | Green（相关提问注入发现；无关提问不注入） |
| RT-UFND-02 待核实标注 | Green（inconclusive 来源行带「待核实」；supported 无标注） |
| RT-UFND-03 否决不注入 | Green（verdict=rejected 的匹配发现不注入） |
| RT-UFND-04 可追溯 | Green（来源行含 finding id、topic、证据 case id） |
| RT-UFND-05 不自动晋升 | Green（注入前后 `desktop_memories` 行数不变、findings 行 updated_at 不变） |

## Red → Green

```text
# Red（发现来源未接线）
cargo test -p an-server --test introspection_materials --locked
→ 4 failed / 1 passed（业务断言：相关提问应注入发现 / 待核实标注 / 来源行含证据 / 不自动晋升前先证注入发生）

# Green（实现后）
cargo test -p an-server --test introspection_materials --locked → 5 passed
```

## 命令与结果

```text
# America/Vancouver 2026-11-23
cargo fmt --all -- --check；cargo clippy --locked --all-targets -- -D warnings → 通过
cargo test --locked --workspace --no-fail-fast → 全绿（既有本机失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
cd web && npm run typecheck && npm test && npm run test:map && npm run build → 通过
E2E 回归：desktop ADMEM 2 passed、session-materials 1 passed
```

## 非本卡

内省页→会话引用回链 UI（第 1 线）；adoption 形态 json 补 case_ids（INT 后续卡）；5.4 组织范围（等身份）。
