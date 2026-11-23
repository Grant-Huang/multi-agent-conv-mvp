# 证据：T-82 首页卡接真实快照（AD-G7 / GET /api/desktop/v1/snapshot）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户批准 T-82 任务卡（对话内；desktop_api.rs 新公开端点方案已批；store.rs 零改动）
- **Gap / Mock**：SMT-03；MOCK_REGISTRY **M1**「首页卡仍本地」收口（`draftStore` 仍本地，后置）
- **范围**：`crates/server/src/desktop_api.rs`（只读端点 + fixture 种子路由）；`web/src/desktop/DesktopHome.tsx`、`web/src/api.ts`；`crates/server/tests/desktop_snapshot.rs`（新增）；登记文档
- **模式**：真实 HTTP；fixture 种子（SMT-03/产品改进）标注「模拟」；加载失败显示明确不可用 + 重试，**不静默回退静态卡**（宪章原则 8）
- **工作分支**：`cursor/ai-desktop-t82-home-snapshot`（独立 worktree）

## 设计要点

- 三区读模型：我的工作=非执行中任务、Agent 工作=Queued/Running、为你关注=Attention 队列（真实读模型，不种 attention）；Verifying 归「我的工作」（等人核验，对齐 A 阶段口径）。
- fixture 种子沿用 mockData 的稳定卡 id（`card-smt-03`/`card-product`，E2E testid 契约不变）且时间戳固定在过去——**不得抢占「最新任务」类逻辑**（S1 委派芯片、历史）。
- fixture 不种 attention；响应含 `counts.tasks_active/attention_unread` 与 `cursor`（AD-G7 §5 形状）。
- store.rs 零改动：数据源为既有 `list_tasks` / `list_desktop_attention`。

## Case

| Case | 场景 |
|---|---|
| RT-ADSNAP-01 | fixture reset 后快照形状契约（三区、模拟标注、source_refs、计数、游标） |
| RT-ADSNAP-02 | 真实任务行出现在快照且 `simulated=false`；fixture 卡保持 `simulated=true` |
| RT-ADSNAP-03 | fixture reset 幂等可复现 |
| RT-ADSNAP-04 | 空库结构有效（不报错、不静默回退静态卡） |
| E2E-ADNAV 等 | `desktop.spec` **31/31 全绿**（断言零改动，仅 beforeEach 增加 fixture 播种） |

## 命令与结果

### Red（实现前）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_snapshot
failures: rt_adsnap_01..04 （4 failed; 路由缺失，业务断言失败，0.05s）
```

### Green（实现后）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test desktop_snapshot
test result: ok. 4 passed; 0 failed ... finished in 0.03s

$ node playwright/cli.js test e2e/desktop.spec.ts     # web/，fresh build 后
31 passed (20.8s)
```

fmt / clippy `-D warnings` / typecheck / vitest 31 / test:map / build 全绿（详见 PR 正文）。

## 排障记录（对后续会话有用）

1. 首轮 E2E 大面积失败的两个根因：① fixture 任务为「最新任务」抢了 S1 委派芯片的 `latest-task` 位 → 种子时间戳固定过去；② **`web/dist` 是共享树里前一会话 00:39 的陈旧构建**（gitignore 不入库，含未合入的「委派默认关联 TASK-2026-018」WIP 逻辑），E2E 一直在验证别人的半成品前端 → **先 `npm run build` 再跑 E2E**，重跑后 31/31、耗时从 9.6m 降到 20.8s。
2. `results.json` 的 `expected`=通过、`unexpected`=失败。

## 非目标

- 不做 `draftStore` 上云（M1 剩余项，另卡）
- 不做快照增量游标（`cursor` 首版为生成时间，AD-G7 的游标续读属 B 后续）
- 不做卡片个性化排序/推荐理由（DESIGN §2 的结构化推荐模型另卡）
