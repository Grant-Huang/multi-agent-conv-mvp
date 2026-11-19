# 阶段 3：Bug / 事故 → 测试 / Eval 闭环

## 纪律

每个高优先级 finding、生产事故或回滚，必须落到下列之一，否则围栏周检记为缺口：

1. `docs/testing/CASES.md` 新用例 + 可执行测试（浏览器 / HTTP / Runtime）
2. `evals/` 评测样本（路由、外发等）
3. 明确标记 `wontfix` / `rejected`（人在台账里签字）

## 流程

```
docs/review/findings.jsonl  opened
        │
        ▼
修复 PR（FIX.md）：Red → Green
        │
        ▼
台账 fix_claimed → 夜间评审 verified_fixed
        │
        ▼
若属误判 / 事故类：写入 CASES 或 evals，并更新 feature-map / COVERAGE
```

## L0 自动合并

- **仅在**重命名绕过已修复（`tools/ci` 使用 `--no-renames`）之后考虑。
- 仅文档 / 评测 / 注释等真 L0；L2/L3 永远不自动合并。
- 开启前再开独立决策条目；本阶段默认 **不开**。

## 夜间浏览器 E2E

工作流：`.github/workflows/nightly-e2e.yml`

- 定时跑 `cd web && npm ci && npm run verify`
- 失败时开 issue（标签 `nightly-e2e`）
- 稳定后再考虑升为 PR 必需检查（另开决策）
