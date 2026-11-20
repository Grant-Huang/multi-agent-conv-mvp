# 运营桌面 / 运营频道 D0b（服务端接线与前端卡片）：证据

日期：2026-11-19。

## 新增运行时用例 RT-OPSDESK-09（未读）
- 先放骨架（`unread()` 恒为 0、`mark_read()` 空操作），`cargo test -p an-runtime --test ops_desk rt_opsdesk_09` 在业务断言上失败：发出第一张卡后未读应为 1，实际为 0（Red）。
- 实现后 7 个 `ops_desk` 测试全部通过。夹具里有一处顺序调整：批准要在案例证据过期前做（引擎报“evidence snapshot stale”），所以改成紧接着发卡后批准；断言目的不变。

## 浏览器用例（`web/e2e/ops.spec.ts`，命令同 D0 的 Red 证据，用本机浏览器缓存）
| Case | 结果 | 说明 |
|---|---|---|
| E2E-OPSCHANNEL | 通过 | 未读 1、打开后清零、只一张带“模拟”的卡、批准后同一张卡更新出批准人、消息条数不变、无模型回复、无任务 |
| E2E-OPSOFF | 通过 | 基线，开关默认关闭时行为不变 |
| E2E-OPSNOISE | 仍失败 | 频道部分的断言都通过（卡片 10 张、汇总卡“另有 3 条”）；失败在最后一步点击“运营桌面”页签——桌面属于 D1，尚未实现。该用例按冻结的断言保留，D1 转绿 |
| E2E-OPSDESK、E2E-OPSDRAWER | 仍失败 | 依赖 D1 的桌面页签 |

## 完整门槛
- `cargo fmt --all -- --check`：通过。`cargo clippy --all-targets`：无警告。`cd web && npm run build`、`npm test`（21 passed）、`npm run test:map`：通过。
- `cargo test --no-fail-fast`：只有 `an-server --test admission::rt_adm_14_admission_http_end_to_end`（“任务未结束”）在本机失败；该用例在干净的 `origin/master` 本机同样失败，而 CI 上通过，属本机环境问题，与本改动无关。
