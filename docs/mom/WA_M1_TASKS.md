# World Adapter M1 执行任务卡

本文件是 [WORLD_ADAPTER_PLAN.md](WORLD_ADAPTER_PLAN.md) **M1 远程 DES 读取**的执行版。硬约束仍见该文 §0（DES-32）；协议落地见 [WORLD_ADAPTER.md](WORLD_ADAPTER.md) §6.1；Mock 替换见 [WA_MOCK_REGISTRY.md](WA_MOCK_REGISTRY.md) WAM-01～03。M0 已准出（#175）。

**准出（缺一不可）**：主应用 MOM 复用 `with_des` / `DesConnector`；世界绑定到远程 DES 运行；Observe/Subscribe 读的是 **MOM 正式投递**边界（非裸 `events_poll`）；试跑零投递、正式运行可投递；WA-01、02、07 的真实 DES 场景有可执行断言；**写路径仍走隔离 Mock**；全量 M0 套件回归绿。

## 0. 进度总表

| 卡片 | 内容 | 状态 | 依赖 |
|---|---|---|---|
| M1-0 | 冻结用例草案与任务卡；勾选 M0 完成；同步 PLAN / Feature Map | 已完成（#176） | M0 准出 |
| M1-1 | 正式 CASES + Red 空壳；随后接线世界绑定 → `DesConnector`（高风险先方案） | Red #178；Green：`/api/world/des`（本卡） | M1-0 |
| M1-2 | 正式投递水位：tick 后发布快照/公开事件；Observe/Subscribe 续读无漏无重 | 未开始 | M1-1 |
| M1-3 | 实验控制接远程创建/推进；试跑隔离；正式运行事项投递运营频道 | 未开始 | M1-1 |
| M1-4 | 浏览器：WA-01/02/07 远程 DES 场景（正式 E2E id）+ M0 全量回归 | 未开始 | M1-2、M1-3 |
| M1-5 | 证据、注册表状态（WAM-01～03 读路径）、准出检查 | 未开始 | M1-4 |

一张卡 ≈ 一个 PR。每卡先 Red（业务断言失败）再 Green；断言冻结后不得弱化。

## 1. 用例命名（登记前）

设计项 WA-01、02、07 在远程 DES 上的验收需独立于 M0 Mock 路径。写入 `CASES.md` 表格前用 `E2E-[A-Z]+` / `RT-[A-Z0-9-]+`。建议映射（M1-0 草案，**待用户确认后改表格 + 空壳**）：

| 设计 | 正式 id | 要点 |
|---|---|---|
| WA-01（远程绑定） | `E2E-WADES` | 会话/工作台绑定远程 DES 世界；跨世界不泄露；`provenance` 可辨 DES |
| WA-02（续读） | `E2E-WAREAD` | 断线或过期游标后重连/重新读取；同事件 ID 只应用一次；水位单独显示 |
| WA-07（观测） | `E2E-WAOBS` | 远程推进后可见正式投递的设备/质量观测；不含审核标准答案 |
| 合同：绑定 | `RT-WADES-01` | `DesConnector` 创建运行后世界绑定可读；未连接明确错误 |
| 合同：投递边界 | `RT-WADES-02` | tick 完成投递后 Observe 才见新事件；中断后服务最近完整边界 |
| 合同：续读 | `RT-WADES-03` | Subscribe 续读无漏、无重复；游标过期 → `CURSOR_EXPIRED` |
| 合同：试跑隔离 | `RT-WADES-04` | 试跑 `delivery_count=0`；正式运行投递 ≥1 且有 `case_id` |
| 回归 | 既有 `RT-DESC-*`、`./scripts/wa-m0-suite.sh` | M1 合入前全绿；写路径仍 Mock |

M0 的 `E2E-DES*` / `E2E-WABIND` / `E2E-WACHANNEL` / `RT-WA-01～09` **不得弱化**，作为全量回归基线。断言已确认并写入 CASES 表格（M1-1 Red）。

## 2. M1-0：冻结与收尾

1. 本文件落地；`docs/PLAN.md` 勾选 M0-6 / M0，增加 M1 执行卡入口。
2. `WA_M0_TASKS.md` 将 M0-6 标为已完成（#175）。
3. ~~草案 bullet~~ → **已完成**：正式表格 + 空壳（`wa-des-remote.spec.ts`、`mom_world_des.rs`）。
4. Feature Map / `WA_MOCK_REGISTRY` 指向本文件与「M1 读路径待替换」。
5. 下一步：接线 `/api/world/des`（改 `engine` / `DesConnector` 前先出方案）→ Green。

## 3. M1-1～M1-3：服务端（纲要）

- 优先在服务层适配：主应用 MOM 启动路径复用 `with_des` / `DesConnector`（见 WORLD_ADAPTER.md §6.1）。
- **高风险**：若需改 `engine.rs` 状态机或 `store`/`orchestrator`，先出方案等人批准（`AGENTS.md`）。
- 公开读边界 = MOM 正式投递后的快照与事件水位；MCP `events_poll` 仅连接器内部搬运。
- 写路径（Propose/Execute/Confirm 真工厂）属 **M2**；M1 保留隔离 Mock 写。
- 故障与时钟测试控件不得在生产配置默认打开。

## 4. M1-4 / M1-5：自动化与准出

- Playwright：远程 DES fixture（假 MCP 或测试进程内连接器）+ 既有 `waMockServer` 写路径对照。
- 每条 `E2E-WADES` / `WAREAD` / `WAOBS` 与 `RT-WADES-*` 可独立重跑；失败留 Trace/关联 ID。
- 合入前：`./scripts/wa-m0-suite.sh` + 新远程套件 + `RT-DESC` 相关回归。
- 更新 `WA_MOCK_REGISTRY`：WAM-01～03 读路径状态；证据 `docs/testing/evidence/wa-m1-*.md`。

## 5. 不做（M1）

- 不接真实工厂写动作 / 子动作回执闭环（M2）。
- 不做生产站点适配（M3/M4）。
- 不退役 `/api/world/mock` 回归路径。
- 不弱化 M0 已冻结断言。
