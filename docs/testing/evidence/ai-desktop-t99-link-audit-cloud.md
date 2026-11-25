# 证据：T-99 关联审计上云（localStorage → SQLite 服务端权威）

- **日期**：2026-11-25（America/Vancouver）
- **授权**：#1 发号 T-99 并批准开工（对话）
- **Gap**：关联修正审计（AD-SCOPE「建立/撤回关联可追溯」）原为 localStorage 本地留痕（清存储/换设备即丢）
- **范围**：`store.rs`（`desktop_task_link_audits` 表 + append/list）；`desktop_api.rs`（`GET/POST /tasks/{id}/link-audits`）；`draftStore.ts`（审计函数改异步 API 版）；`WorkspaceComposer.tsx`（audits 状态源改服务端拉取+追加后刷新；失败显式报错 `scope-audit-error`）；`desktop_link_audit.rs`（新增）；登记文档
- **模式**：真实 HTTP + SQLite；localStorage 版本移除（审计丢失不可接受，原则 3）；失败显式报错不静默
- **实现说明（并行收敛）**：本卡的实现在并行会话分支 `3b4fcde` 上完成并推送（同名测试/同表结构——双方均按 CASES RT-ADLAUD 验收行实现，自然收敛）；本文件由主线 #1:1 在该分支上补齐登记文档并做独立验证。`rt_adlaud_03/04` 为本线补写的两例（非法值拒绝 / 重启持久+跨库隔离）。

## Case

| Case | 场景 |
|---|---|
| RT-ADLAUD-01 | 追加/列表回环（服务端 id、created_at 升序、字段一致） |
| RT-ADLAUD-02 | 按 task 过滤互不可见 |
| RT-ADLAUD-03 | action 非法值 400 拒绝 |
| RT-ADLAUD-04 | 同库重启持久 + 全新库为空（H4 同族探针） |
| E2E-ADSCOPE | 既有「撤回关联记录可追溯」断言零放松，复跑通过 |

## 命令与结果

### 实现验证（其分支 3b4fcde，`desktop_link_audit` 4 例）

```
$ cargo test --locked -p an-server --test desktop_link_audit
test result: ok. 4 passed; 0 failed ... finished in 0.06s
```

### 回归与前端

```
$ cargo test --locked -p an-server --test desktop_read --test desktop_fixture_gate --test desktop_replay --test restart_semantics
6 / 2 / 5 / 11 全绿
$ desktop.spec：32 passed
$ test:map：33 features, 93 browser cases ✓
fmt / clippy -D warnings ✓
```

全量 workspace 与 10 套件 E2E 结果见 PR 评论（并发窗口避让说明）。

## 非目标

- 与 RT-ADLINK 服务端关联表的合并展示（另卡）；多用户隔离（后置）；Attention 深化（另卡）

## 出问题先看

- 审计面板报错 `scope-audit-error`：服务端不可达或 action 非法
- 列表跨任务串写：检查 `task_id` 过滤（RT-ADLAUD-02）
