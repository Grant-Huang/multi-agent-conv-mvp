# 首页真实委派切片：S1 接 B1 受理 + GS3 日常入口

| 字段 | 内容 |
|---|---|
| 状态 | 有效 |
| 日期 | 2026-11-23 |
| 来源 | 用户授权下一卡（对齐 INTEGRATION_PLAN 近端第 1/4 线） |
| 关联 | E2E-HOMEACCEPT、E2E-HOMEREV、E2E-ADASK；T1/S7；WP2 #215 |

## 背景

B1 可靠受理与 WP2 `review-verdict` 已在 API/经典壳合入；桌面 S1 委派仍为 `setTimeout` Mock，GS3 无法从日常入口验收。

## 决策

1. **不改** `orchestrator.rs` / `store.rs` / `registry.rs`：复用 `desktopAcceptCreate`/`Get` 与现有 `reviewVerdict` + ChatView/TaskPanel。
2. S1「委派任务」走 `durableHomeAccept`（POST 后只信 GET；同 op 幂等复验）→ 等会话任务落库 → 芯片显示真实 `task-*`。
3. 「打开任务」进入**发起会话**（聊天），而非 SMT-03 Mock 工作区；验收按钮沿用主路径「判定通过」。
4. 用例：`E2E-HOMEACCEPT`、`E2E-HOMEREV`；同步放宽 `E2E-ADASK` 断言到真实 task id。

## 非目标

WP5 反馈→评测、WP7 重启、WP6 准入、WP1/WA、WP8 六黄金合并。

## 回滚

恢复 S1 Mock 委派与 ADASK 原断言；删除 `homeDelegate.ts` 与新 E2E。
