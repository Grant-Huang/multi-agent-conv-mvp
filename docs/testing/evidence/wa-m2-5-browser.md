# World Adapter M2-5 — 浏览器远程写场景（T-58）

分支：`cursor/wa-m2-5-browser-e2e-f2df`。方案：[WA_M2_5_PLAN.md](../../mom/WA_M2_5_PLAN.md)。沿用已批服务层路径。

## 范围

- Playwright Green：`E2E-WAEXEC` / `WARECV` / `WADYN` / `WALOOP`
- 服务层：`allow_test_controls`（harness）、`official` 支持 `world_id`、Propose 对齐修订、Execute/批准投影 OpsCase、`ops/approve-execute`
- 前端：`OpsCards` DES 路径（`execution-id` / `查证结果` / `execution-evidence`）；`DesExperiment` 单步软成功
- 套件：`./scripts/wa-m2-suite.sh` + M0/M1 回归

## Green 证据（VERIFIED / T2）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture
… 12 passed（含 m25_official_by_world_id_delivers_ops）

$ cd web && npx playwright test e2e/wa-des-write.spec.ts
  ✓ E2E-WAEXEC / WARECV / WADYN / WALOOP
  4 passed

$ ./scripts/wa-m2-regression.sh
== wa-m2 regression green ==

$ ./scripts/wa-m1-suite.sh
== wa-m0 suite green ==
== wa-m1 suite green ==

$ CARGO_TARGET_DIR=target cargo test --locked --workspace
… ok（exit 0）
$ CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings
… Finished
$ cd web && npm run typecheck && npm test && npm run test:map && npm run build
… Feature map valid；23 tests；build ok
```

## 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先 | 只扩 `mom_world_des` + `OpsCards` / api / DesExperiment | 符合 |
| 不改 engine/DesConnector | 不改 | 符合 |
| 不断言弱化 | 业务 Then 保持；仅修正 selectOption.label 的 Playwright API（RegExp→string） | 符合 |

## 测试变更说明

`wa-des-write.spec.ts` 中 `selectOption({ label: /DES/ })` 在当前 Playwright 下抛 API 错误（label 须为 string）。改为与 `wa-des-remote` 一致的 `'刀具磨损演练 · DES'`，不改变「选 DES 世界且无标准答案」业务断言。

## 非目标

不改 ChatCard 生成类型；不接生产 MES；不改引擎状态机；M2-6 准出清单另卡。
