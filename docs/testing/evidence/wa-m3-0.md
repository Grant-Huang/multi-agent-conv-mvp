# World Adapter M3-0 — 冻结生产只读任务卡与断言草案

## 范围

- 新增执行卡：`docs/mom/WA_M3_TASKS.md`
- PLAN 勾选 M2-6 / **M2** 准出；增加 M3-0 入口
- CASES 增加 M3 **草案 bullet**（未写表格行，避免 check-map 缺测试）
- Feature Map / Mock 注册表指向 M3 生产读（WAM-07）

## 拟正式 id（待用户确认）

| 设计 | 拟 id | 要点 |
|---|---|---|
| 生产绑定与观测 | E2E-WAPROD | 生产世界只读；provenance=production；写关闭或隔离 |
| 权限与隔离 | E2E-WAPISO | 跨站点/无权限拒绝且不泄露 |
| 断线续读 | E2E-WAPREAD | 游标恢复；同 ID 一次；水位可辨 |
| 多源延迟 | E2E-WAPDELAY | as_of / 延迟可见 |
| 合同 | RT-WAPROD-01～06 | 绑定、水位、权限、续读、延迟、写关闭 |

确认后：M3-1 起改表格 + 空壳 Red，再实现。写路径保持 Mock/DES；真 MES 写属 M4。

## 地图校验

```bash
cd web && npm run test:map
# → Feature map valid: 32 features, 67 browser cases
# （本卡不新增 features.json cases 表格行；仅 sources 挂任务卡/证据）
```

## 非目标

本卡不写业务代码；不改 `engine` / DesConnector 内部；不接真实 MES 写；不弱化 M0～M2 断言。
