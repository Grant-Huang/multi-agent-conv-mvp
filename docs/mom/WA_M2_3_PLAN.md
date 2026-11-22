# M2-3：恢复与幂等加深（服务层）

承接 [WA_M2_TASKS.md](WA_M2_TASKS.md) M2-3、[WORLD_ADAPTER.md](WORLD_ADAPTER.md) WA-04 / §6.2、`E2E-WARECV` 与 `RT-WADES-12/13`。  
**默认仍走已批服务层路径**（决定 `2026-11-21-wa-m2-1-green-service-layer`）；不改 engine / DesConnector 内部。

## 1. 与 M2-2 的关系

| 项 | M2-2 | M2-3 |
|---|---|---|
| RT-WADES-12 | 丢回执 → `unknown`；restore → 确认；部分失败含混合证据 | 按**原关联键** Confirm 回原工单；**部分回执可补齐** |
| RT-WADES-13 | Execute 在工厂重启后 `FACTORY_INSTANCE_STALE` | 旧执行**历史可 Confirm**；pending **不误投**新实例 |
| E2E-WARECV | Playwright 空壳 | 仍属 **M2-5**；本卡只加深合同层 |

## 2. 实现（仅 `mom_world_des`）

1. Execute 登记稳定 `correlation_key`（外部关联键）；Confirm 可用 `execution_id` **或** `correlation_key` 找回同一执行。
2. 测试控件 `complete-partial`：部分失败后补齐剩余子动作回执 → 再 Confirm 得终态。
3. `factory-restart`：抬升实例并清空该世界待发；Confirm 只读历史证据（不因实例变更拒查）；flush 对陈旧 pending `dispatched_count=0`。

## 3. 规范对照

| 规范默认 | 本卡 | 结论 |
|---|---|---|
| 服务层优先，不改 engine/DesConnector | 只扩 `mom_world_des` | 符合（已批路径） |
| 高风险先方案 | 不触及 AGENTS 高风险文件 | 无需再批 |
| E2E 浏览器 Green | M2-5 | 本卡不做 |

## 4. 验收

- 新增合同：`RT-WADES-15`（关联键恢复）、`RT-WADES-16`（部分回执补齐）、`RT-WADES-17`（重启后历史可查 + 不误投）
- `cargo test -p an-server --test mom_world_des_write` 含上述用例绿
- 挂入 `features.json` `runtime_cases`；证据 `wa-m2-3-recovery.md`

## 5. 非目标

不改 engine / DesConnector / store 表 / orchestrator；不宣称 Playwright `E2E-WARECV` Green；不开放跨进程 `durable`。
