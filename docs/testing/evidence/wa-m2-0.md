# World Adapter M2-0 — 冻结远程 DES 写任务卡与断言草案

## 范围

- 新增执行卡：`docs/mom/WA_M2_TASKS.md`
- PLAN 勾选 M1（含 T-50）准出；增加 M2-0 入口
- CASES 增加 M2 **草案 bullet**（未写表格行，避免 check-map 缺测试）
- Feature Map / Mock 注册表指向 M2 写路径（WAM-04～06）

## 拟正式 id（待用户确认）

| 设计 | 拟 id | 要点 |
|---|---|---|
| WA-03 | E2E-WAEXEC | 授权/并发/取消守卫 |
| WA-04 | E2E-WARECV | 丢回执恢复、部分回执、工厂重启 |
| WA-05 | E2E-WADYN | 干预闭环：前缀不变、未来改变 |
| WA-08 | E2E-WALOOP | 双角色产品端到端同一执行证据 |
| 合同 | RT-WADES-09～14 | 提案、并发、取消、恢复、重启、干预 |

确认后：M2-1 起改表格 + 空壳 Red，再实现。读路径保持 M1/T-50；Mock 写路径保留回归。

## 地图校验

```bash
cd web && npm run test:map
# → Feature map valid（本卡不新增 features.json cases）
```

## 非目标

本卡不写业务代码；不改 `engine` / `DesConnector` 内部；不接生产 MES；不弱化 M0/M1 断言。
