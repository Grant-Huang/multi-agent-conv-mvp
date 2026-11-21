# World Adapter M1-0 — 冻结远程 DES 读任务卡与断言草案

## 范围

- 新增执行卡：`docs/mom/WA_M1_TASKS.md`
- 勾选 M0 / M0-6 完成（#175）；PLAN 进入 M1
- CASES 增加 M1 **草案 bullet**（未写表格行，避免 check-map 缺测试）
- Feature Map / Mock 注册表指向 M1 读路径

## 拟正式 id（待用户确认）

| 设计 | 拟 id | 要点 |
|---|---|---|
| WA-01 远程 | E2E-WADES | 绑定远程 DES 世界；隔离；provenance |
| WA-02 | E2E-WAREAD | 续读无漏无重；过期与水位 |
| WA-07 观测 | E2E-WAOBS | 正式投递观测；无标准答案 |
| 合同 | RT-WADES-01～04 | 绑定、投递边界、续读、试跑隔离 |

确认后：M1-1 起改表格 + 空壳 Red，再实现。写路径仍 Mock；`./scripts/wa-m0-suite.sh` 为回归基线。

## 地图校验

```bash
cd web && npm run test:map
# → Feature map valid（本卡不新增 features.json cases）
```

## 非目标

不改 `DesConnector` / `engine.rs`；不接真实写；不弱化 M0 断言。
