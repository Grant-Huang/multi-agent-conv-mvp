# 证据：conversation-memory 5.1 会话资料注入

- **日期**：2026-11-23（America/Vancouver）
- **授权**：五线计划（decisions/2026-11-22-five-workstreams）第 5 线任务「确认 REQ-conversation-memory-01 并建立会话注入验收」；验收标准与默认开关经用户 2026-11-23 对话确认冻结
- **范围**：`crates/runtime/src/session_materials.rs`（新）+ `conversation.rs` 回答路径挂接 + `store.rs` 只读方法 `list_unrevoked_desktop_task_ids` + `[conversation]` 三配置项；未改 orchestrator；未改 HTTP API 契约
- **契约**：RT-SMAT-01～05 / E2E-SMAT（CASES「会话资料注入」，2026-11-23 冻结）

## Case

| Case | 结果 |
|---|---|
| RT-SMAT-01 续问带来源 | Green（提示词含资料块：来源行 task_id·状态·时间 + 摘要；最新用户消息完整） |
| RT-SMAT-02 撤回剔除 | Green（撤回后无资料块，回答照常） |
| RT-SMAT-03 超时/错误降级 | Green（慢源超时、错误源均不失败当前轮、不带资料） |
| RT-SMAT-04 预算裁剪 | Green（超长摘要截到上限内，来源行保留） |
| RT-SMAT-05 日志无正文 | Green（session.materials 只含来源 id/原因） |
| E2E-SMAT | Green（UI 委派 → B1 HTTP 建链 → 出窗 → UI 续问 → 回答含窗口外摘要） |

## Red → Green

```text
# Red（挂接实现前，2026-11-23）
cargo test -p an-server --test session_materials --locked
→ 3 failed / 2 passed（业务断言：应含资料块×2、应记录 session.materials 事件）

# Green（实现后）
cargo test -p an-server --test session_materials --locked
→ 5 passed
```

## 命令与结果

```text
# America/Vancouver 2026-11-23
CARGO_TARGET_DIR=target cargo fmt --all -- --check            → 通过
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings → 无警告
CARGO_TARGET_DIR=target cargo test --locked --workspace       → 全绿（既有本机失败 rt_adm_14 / rt_agcat_01 与本卡无关，见移交记录）
cd web && npm run typecheck && npm test && npm run test:map && npm run build → 通过
PLAYWRIGHT_BROWSERS_PATH=… npx playwright test e2e/session-materials.spec.ts → 1 passed
```

## 决策

- 默认开关 `session_materials = true`（用户 2026-11-23 确认）；`fixture` 无关；无表结构变更。
- 资料范围（本卡非目标）：个人长期记忆（5.2）、INT 发现（5.3）、组织范围（5.4）、判断/路由/执行路径选材。
