# 证据：T-86 draftStore 上云（工作区草稿服务端权威）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户批准 T-86 任务卡（对话内；store.rs/desktop_api.rs 高风险方案即本卡）
- **Gap / Mock**：SMT-03；MOCK_REGISTRY **M1**「`draftStore` 仍本地」收口（首页卡接真为 T-82 #250 在途；`TaskLinkAudit` 审计仍本地，另卡）
- **范围**：`crates/runtime/src/store.rs`（`desktop_drafts` 表 + 读写/清空方法）；`crates/server/src/desktop_api.rs`（`GET/PUT/DELETE /api/desktop/v1/drafts/{kind}/{scope_id}`）；`web/src/desktop/draftStore.ts`（服务端权威 + 本地缓存镜像）、`WorkspaceComposer.tsx`（接线）；`crates/server/tests/desktop_drafts.rs`；登记文档
- **模式**：真实 HTTP + SQLite 持久化；本地缓存为用户自己的数据（离线可用），非 Mock 回退
- **工作分支**：`cursor/ai-desktop-t86-drafts-cloud`（独立 worktree）

## 语义

- version 由客户端携带（本地编辑计数），`expected_version` 防丢失更新：过期 PUT 返回 **409 `version_conflict`** + 当前草稿，不覆盖
- 发送后清理：仅当现存 version == `sent_version` 才清空正文（version+1，scope 保留）；stale 版本幂等（保护发送期间追加的输入，对齐 DESIGN §3）
- 前端合并口径：本会话从未同步过该键且本地已有输入 → 推送本地；本地为空 → 采纳服务端（跨设备/清存储恢复）；已同步过 → 版本新者胜
- 与 `desktop_handoff`（D 卡跨端草稿冲突）语义不同，互不替代

## Case

| Case | 场景 |
|---|---|
| RT-ADDRAFTS-01 | 保存/读取回环（text/scope/continue_task_id/version 一致） |
| RT-ADDRAFTS-02 | expected_version 过期 → 409 + 当前草稿，不覆盖 |
| RT-ADDRAFTS-03 | 发送后按 sent_version 清空；stale 幂等 |
| RT-ADDRAFTS-04 | 同库重启持久（scope/continue_task_id 保留）；kind 非法 400 |
| E2E-ADDRAFT 等 | `desktop.spec` 31/31（断言零改动） |

## 命令与结果

### Red（实现前）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_drafts
4 failed（路由缺失；0.05s 快速失败，非环境问题）
```

### Green（实现后）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_drafts
test rt_addrafts_01_save_load_roundtrip ... ok
test rt_addrafts_02_version_conflict ... ok
test rt_addrafts_03_clear_after_send ... ok
test rt_addrafts_04_restart_persists_and_kind_validation ... ok
test result: ok. 4 passed; 0 failed ... finished in 0.05s
```

### 浏览器与前端回归（fresh build 后）

```
$ node playwright/cli.js test e2e/desktop.spec.ts
31 passed (20.9s)        # E2E-ADDRAFT 断言零改动
$ node typescript/bin/tsc --noEmit    # typecheck OK
$ node vitest/vitest.mjs run         # 31 passed
$ npm run test:map                    # Feature map valid: 33 features, 92 browser cases
$ node vite/bin/vite.js build         # ✓ built
```

全量 workspace：44 个二进制全绿；唯一失败 `rt_adm_14`（10.06s）为已知间歇问题（已在 #250/证据中三处立证移交 admission 负责人，与本卡无关）。

## 开发期发现并修复的设计缺陷（对后续会话有用）

首版合并规则「服务端版本 ≥ 本地 → 覆盖本地」在**新会话刚开始输入**时会把用户刚敲的字用旧会话的服务端副本打掉（版本号跨会话不可比；E2E-ADMEM/ADMEMW/ADPROF 由此假红）。修正为按「本会话是否同步过该键」区分：未同步且本地已输入 → 回推本地；本地为空 → 采纳服务端；已同步 → 版本新者胜。

## 非目标

- `TaskLinkAudit` 关联审计上云（另卡）
- 与 `desktop_handoff` 跨端草稿合并策略打通（D 卡已覆盖其场景）
- 快照增量游标、多用户草稿隔离（principal 过滤后置）
