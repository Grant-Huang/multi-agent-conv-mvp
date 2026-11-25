# 证据：T-96 助理消息已读持久化（T-73 进程内夹具 → SQLite 两表）

- **日期**：2026-11-24（America/Vancouver）
- **授权**：#1 发号 T-96 并批准开工（对话）
- **Gap**：MOCK_REGISTRY M3「已读跨端夹具（T-73；无新表）」的持久化收口——`AssistantReadState` 为进程内 OnceLock，重启即丢
- **范围**：`store.rs`（`desktop_assistant_messages` + `desktop_assistant_read_ids` 两表，新库播种）；`desktop_api.rs`（4 个处理器夹具→store，响应形状不变）；`desktop_read.rs`（RT-ADXR-05/06）；登记文档
- **模式**：真实 HTTP + SQLite；前端零改动；fixture/reset 端点保留（清已读集）
- **工作分支**：`cursor/ai-desktop-t96-assistant-reads-persist`（独立 worktree）

## 语义（忠实于 T-73）

- 消息内容静态（3 条种子，新库播种一次）；已读集合独立成表（**未知 id 亦可标已读**——忠实于 T-73 的 `read_ids.insert` 语义，不做存在性校验）
- reset = 清已读集（全部回到未读）；标记幂等（重复标 `newly_marked=0`）
- 表按 (库) 隔离：count==0 播种，软删不存在 → 不发生「清空后重播种复活」（对比 T-81 探针结论：count 语义安全）

## Case

| Case | 场景 |
|---|---|
| RT-ADXR-05 | 同库重启持久：标记已读后同库重开服务，已读/未读数保留 |
| RT-ADXR-06 | 库间隔离：全新库不继承其他库/进程的已读状态（H4 同族探针） |
| RT-ADXR-01～04 | 既有基线，断言零放松，全绿复跑 |

## 命令与结果

### Red（实现前）

```
$ cargo test --locked -p an-server --test desktop_read
5 passed; 1 failed
rt_adxr_06_fresh_db_does_not_inherit_process_state:
  assertion: msg1_b["read"] == false → "进程级已读状态泄漏到了另一个全新库"
```

### Green（实现后）

```
$ cargo test --locked -p an-server --test desktop_read
test result: ok. 6 passed; 0 failed ... finished in 0.11s
```

### 回归与前端

```
$ cargo test --locked --workspace：44 个二进制全绿（唯一失败 rt_adm_14 间歇指纹，已移交）
$ cargo test --locked -p an-server --test desktop_read --test desktop_fixture_gate --test desktop_replay --test restart_semantics
  6 + 2 + 5 + 11 全绿
$ desktop.spec：32 passed（38.4s；ADD01-02 首跑偶发失败，单跑复现通过——D 线跨端用例，与本卡改动无关）
$ test:map：33 features, 93 browser cases ✓；fmt/clippy ✓
```

## 非目标

- 记忆建议流（DESIGN §7，牵宪章原则 10 口径，另议）；Attention 静默时段/频控（深化另卡）
- TaskLinkAudit 上云（另卡）；多用户隔离（principal 夹具后置）
- assistant/messages/fixture/reset 生产门控（同 H4 类问题，未在本卡批准范围——建议随下一次夹具治理统一处理，已在 PR 说明）

## 出问题先看

- 重启后已读丢失：检查 `desktop_assistant_read_ids` 表是否有行（播种只写 messages 表，read_ids 独立）
- 列表为空：确认非新库（新库首次 list 播种）——种子内容见 `store.rs desktop_assistant_seed_rows`
