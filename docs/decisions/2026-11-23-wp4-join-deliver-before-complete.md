# WP4 汇合：先交付再标父任务 COMPLETED

| 字段 | 内容 |
|---|---|
| 状态 | 有效 |
| 日期 | 2026-11-23 |
| 来源 | 热修授权（CI 抖动；用户允许「等齐交付消息或修编排竞态」） |
| 关联 | RT-TPL-12；`orchestrator::complete_parent`；#222 后 rust CI |

## 背景

`rt_tpl_12_all_ok_join_completes_parent` 在 CI 偶发 panic「父任务应有一次交付消息」。本地/同套件其余 RT-TPL-11/13～16 稳定。

## 事实

`complete_parent` 原顺序：`finish(COMPLETED)` → `deliver()`（Composer 对不可达 LLM 回退有连接耗时）。测试在观测到父终态后立即读会话消息，与交付写入竞态。

## 决策

1. **编排**：`complete_parent` 改为先 `deliver`，再 `finish(COMPLETED)`，使「终态可见」蕴含「交付已写入」。
2. **测试**：RT-TPL-12 在断言前轮询等齐 assistant 消息；断言目标不变（仍须有交付），不弱化。

## 非目标

不改 store/DDL、不改 Composer 回退策略、不扩大 WP4 范围。

## 回滚

恢复 `finish`→`deliver` 顺序，并去掉测试轮询（会再暴露原竞态）。
