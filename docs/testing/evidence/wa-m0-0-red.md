# World Adapter M0-0 — DES-UI 用例冻结与业务 Red

## 范围

将设计编号 DES-UI-01～06 冻结为正式浏览器用例 `E2E-DESTRIAL`～`E2E-DESLOOP`，写入 `docs/testing/CASES.md` 表格，登记 `features.json` 的 `world-adapter`，并提供 Playwright 空壳（`web/e2e/des-experiment.spec.ts`）。

## 断言冻结

| 设计 | 正式 id | 要点 |
|---|---|---|
| DES-UI-01 | E2E-DESTRIAL | 试跑零投递；非法参数提示 |
| DES-UI-02 | E2E-DESSTART | 正式启动幂等；试跑≠正式 ID |
| DES-UI-03 | E2E-DESOPS | 运营频道内批准与执行 |
| DES-UI-04 | E2E-DESPAUSE | 暂停等待≠失败；推进回执 |
| DES-UI-05 | E2E-DESISO | 隔离与跨运行拒绝 |
| DES-UI-06 | E2E-DESLOOP | Mock 双角色闭环 |

断言定稿后不得弱化；实现改业务代码迎合断言（见 `WA_M0_TASKS` M0-1～M0-5）。

## Red（实现前）

期望：`npx playwright test des-experiment.spec.ts` 在默认 harness 上**业务断言失败**（缺少「DES 仿真实验」入口、试跑/正式运行控件、运营投递合同等），不是编译或环境超时。

转绿依赖：M0-1 Mock 合同 → M0-2/3/4 前端 → M0-5 双角色 Harness。物理闭环断言随 M2。

## 地图校验

```bash
cd web && npm run test:map
```

须通过：`world-adapter` 六条 case 与 `des-experiment.spec.ts` 中 `test('E2E-DES…` 一一对应。
