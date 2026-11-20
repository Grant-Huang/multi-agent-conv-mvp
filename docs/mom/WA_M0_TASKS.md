# World Adapter M0 执行任务卡

本文件是 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) M0 的执行版。硬约束见该文 §0（DES-32）；入口拆分见 [DES_EXPERIMENT_OPS_SPLIT.md](DES_EXPERIMENT_OPS_SPLIT.md)（决定 62）。通用流程、完成的定义见 [OPS_DESK_TASKS.md](OPS_DESK_TASKS.md) §2。

**准出（缺一不可）**：完整前端可演示双角色全流程；后端未就绪用 Mock；DES-UI-01～06 与适用的 WA/UI 断言在 Mock 环境可自动跑；Mock 开发与替换表（PLAN §4）已落地并可切换记录。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| M0-0 | 冻结用例草案 → CASES / Feature Map 正式名；同步 features.json 占位 | 已完成（#159） | DES-32、决定 62 |
| M0-1 | 协议 Mock + 实验控制 Mock（试跑/正式运行/时钟/投递）HTTP 合同 | 已完成（#161） | M0-0 |
| M0-2 | DES 仿真实验前端（配置、试跑、正式运行、结果） | 已完成（#164） | M0-1 |
| M0-3 | AI 桌面运营频道联动（正式运行发卡、方案/执行/核验） | 本 PR：E2E-DES* 六条 Green | M0-1；复用 D0～D2 |
| M0-4 | 三入口工作台 + 事项页（看板已有部分可复用） | 未开始 | M0-1 |
| M0-5 | Playwright：DES-UI-01～06 + 适用 WA/UI；集成合同套件 | 未开始 | M0-2～M0-4 |
| M0-6 | 全量回归与证据；准出检查 H1～H4 | 未开始 | M0-5 |

一张卡 ≈ 一个 PR。每卡先 Red（业务断言失败）再 Green；断言冻结后不得弱化。

## 1. 用例命名（登记前）

Feature Map 中的 `DES-UI-01～06`、`E2E-WA-*` 为设计编号。写入 `CASES.md` 表格前须换成 `check-map` 允许的浏览器用例 id：`E2E-[A-Z]+`（无数字、无连字符）。建议映射（M0-0 定稿）：

| 设计编号 | 拟正式 E2E id | 要点 |
|---|---|---|
| DES-UI-01 | `E2E-DESTRIAL` | 配置试跑；运营频道新增卡为零 |
| DES-UI-02 | `E2E-DESSTART` | 正式启动幂等；试跑≠正式 ID |
| DES-UI-03 | `E2E-DESOPS` | 运营员桌面内批准与执行 |
| DES-UI-04 | `E2E-DESPAUSE` | 暂停/单步；等待≠失败 |
| DES-UI-05 | `E2E-DESISO` | 隔离与跨运行授权拒绝 |
| DES-UI-06 | `E2E-DESLOOP` | 双角色闭环（Mock 业务路径） |

WA/UI 映射沿用 [world-adapter.md](../feature-map/features/world-adapter.md)；实施时写入 CASES 表格并配测试文件后才进 `features.json` 的 `cases` 数组。

## 2. M0-0：冻结与登记

1. 将 DES-UI-01～06 断言写成 CASES **草案 bullet**（勿先写表格行，避免 check-map 报缺测试）。
2. 用户确认断言后，改成表格行 + 空壳 Playwright（业务 Red）。
3. 更新 Feature Map 控件名与正式 id；`docs/testing/features.json` 增加 `world-adapter` 功能条目（可先只挂 `runtime_cases` 空或文档源）。
4. 同步本文件 §0 与 `docs/PLAN.md` World Adapter 进度。

## 3. M0-1：服务端 Mock

- 新路由（建议独立模块，如 `mom_world_mock.rs`）：世界绑定、Observe/Subscribe/Propose/Execute/Confirm、实验配置/试跑/正式运行/时钟、事项投影到运营发布器。
- 可控时钟与故障注入仅测试进程开放。
- 试跑与正式运行分实例；试跑投递计数恒为 0。
- 合同测试：重复启动幂等、跨运行拒绝、游标过期、丢回执/部分失败。
- **不改** `engine.rs` 状态机除非另批方案；优先在服务层适配。

## 4. M0-2 / M0-3 / M0-4：前端

- M0-2：实验页四视图（配置/试跑/正式运行/结果）；可访问名称稳定。
- M0-3：复用 `OpsCase` 与运营频道；正式运行事项发卡；审批仍只在频道卡（决定 60）。
- M0-4：看板/任务/资源与事项页；与实验、频道共用 `case_id` / `world_id`。
- 真实/DES/Mock 在视图与记录级可辨。

## 5. M0-5 / M0-6：自动化与准出

- Playwright：双浏览器上下文（实验员 / 运营员）；Harness 装载种子与时钟。
- 每条 DES-UI 与适用 WA/UI 有独立可重跑用例；失败留 Trace/截图/关联 ID。
- 准出检查表：H1 完整前端；H2 Mock 全流程；H3 Mock 表可演示替换钩子；H4 清单全覆盖。
- 证据写入 `docs/testing/evidence/wa-m0-*.md`。

## 6. 不做（M0）

- 不接真实 DES MCP 写路径（属 M1/M2）。
- 不做生产站点适配（M3/M4）。
- 不退役 `/mom/`（仅标注沙盘；退役另卡）。
- 不让会话模型直接审批/执行。
