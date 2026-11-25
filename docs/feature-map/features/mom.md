# mom：MOM 运营沙盘、车间 DES 与模拟工厂（全部为模拟）

覆盖 features.json 的 mom、mom-des、des-factory、mom-workbench-prototype。**所有业务系统、主 Agent、审批身份、DES 参数都是替代件**，清单见 [MOCK_REGISTER](../../mom/MOCK_REGISTER.md)；通过只证明控制链行为，不证明真实工厂的诊断/排产正确。

## Sub-features

| 子功能 | 说明 |
|---|---|
| 公告板与案例 | 独立 SQLite、事务事件/outbox、DAG、资源版本审批、幂等下发、租约与隔离 |
| 两个场景 | S1 产线多扰动（审批后下发、核验再关闭）；S2 客诉（先初步回复，8D 缺证据不能结案） |
| 双视图 | 人类运营视图 / Agent 机器视图（JSON，含 `next_cursor`） |
| 配置持续仿真 | 订单数量、随机种子、持续事件数量与间隔；播放 30 分钟 / 暂停；重启恢复 |
| 安全联锁 | 触发后 HTTP PATCH/下发被拒且无副作用；终态案例拒绝手动故障注入 |
| 车间 DES | 开环→闭环（执行回写仿真世界）、基于可观测状态的 what-if、协同事项（同线 60 分钟内多案例）、审核层（规则 + AI 审核降级 + 人工确认）、批量实验 CSV |
| 模拟工厂与 MCP | `crates/des` + `crates/des-mcp`：13 个工具（观测/执行/控制/管理），写操作需幂等键与审批引用 |
| RelOS 上下文 | `an-mom --relos-url`：S1 诊断读取 `/v1/context/compile`，不可用时安全降级 |
| 设计原型 | `docs/mom/prototype/`，浏览器直接打开 |

## How to get to it (user POV)

同源 `/mom/` 挂载直达（页面标题“MOM 运营公告板”；T-88 移除主界面“MOM 运营沙盘”入口）；或独立运行 `cargo run --bin an-mom`（<http://127.0.0.1:8790>，必须回环地址）。

## Driving it with Playwright / HTTP

```ts
await page.goto('/mom/');   // T-88 移除主界面入口，直达同源挂载
await page.getByRole('button', { name: '＋ 多扰动 S1' }).click();
await expect(page.locator('#content h2').first()).toContainText('WAITING_HUMAN');
await page.getByRole('button', { name: '人类运营视图' }).click();
await page.getByRole('button', { name: '批准', exact: true }).click();
await page.getByRole('button', { name: '执行已批准动作' }).click();      // → VERIFYING
await page.getByRole('button', { name: '推进 5 分钟' }).click();         // 虚拟时钟
await page.getByRole('button', { name: '核验并关闭案例' }).click();      // → CLOSED
```

S2：“＋ 客诉 S2”；错误区 `#error`；播放状态 `#playback-state`；配置表单标签“订单数量”“随机种子”“持续事件数量”“事件间隔（仿真分钟）”，提交按钮“创建配置仿真”。独立服务的 E2E 用端口 **18790**。

HTTP（`crates/server/src/mom.rs`）：`GET /api/mom/cases`、`POST /api/mom/scenarios/{s1|s2}`、`POST /api/mom/playback`、`GET /api/mom/gaps`（23 项缺口详情）、`GET /api/mom/cases/{id}`（人类）/`/agent`（机器）/`/events`、`POST /api/mom/cases/{id}/commands`、`POST /api/mom/events`。

```bash
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom          # RT-MOM-*
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des      # RT-DES-*
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom           # HTTP-MOM-*
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des       # HTTP-DES-*
python3 tools/mom/smoke.py --report .tmp/mom/final-smoke-report.json          # 13 个 HTTP 断言
```

## Verify（判据）

- S1：批准前没有下发；执行后进入 VERIFYING；推进时钟后出现核验证据（`mock-verification`）；关闭后 `CLOSED`。
- S2：页面出现 `INITIAL_RESPONSE`、`UNKNOWN`、`PROGRESS_8D`；对客动作 `customer.send` 只写模拟发件箱；缺证据关闭 → `#error` 含 “verification incomplete”，状态不为 CLOSED。
- 配置影响排产：排产方案区出现 `"quantity": 900`；播放后“播放中”，暂停后“已暂停”。
- DES：同种子可复现；标准答案与观测隔离；超大场景 2 秒内 400 而不是卡死；关闭协同事项时安全联锁成员被跳过并保留预留。

## Gotchas

- **模拟环境**：不要用这里的通过结果推断真实 MES/QMS/EAM/WMS 行为；22 个 MOCK、1 REAL_UNVERIFIED。
- AI 审核模型未接入：审核接口只返回规则结果（“no reviewer configured; rules only”）。
- 审批身份是测试身份（如 `supervisor`），没有真实认证与 RBAC。
- “推进 5 分钟”推进的是**虚拟时钟**；SLA 使用持久 deadline，不具备墙钟调度。
- 页面是独立前端（`web/mom/`、`#content`/`#error` 等 id），与主应用的 React 组件无关；改一边不会影响另一边。
- 重复路由注册、尾斜杠回退曾导致主界面入口问题，改 `/mom/` 路径相关逻辑要回归 E2E-MOMDYNAMIC。

## Backed by

源码：`crates/runtime/src/mom/`（含 `des/`）、`crates/server/src/mom.rs`、`mom_sim.rs`、`mom_relos.rs`、`bin/an-mom.rs`、`crates/des`、`crates/des-mcp`、`web/mom/`
用例：E2E-MOM、E2E-COMPLAINT、E2E-MOMDYNAMIC；RT-MOM-01～10、RT-DES-01～35、RT-DESF-01～10、RT-DESM-01～04、HTTP-DES-01～05、RT-BIND-01～10；P-MOM-*（原型）
文档：`docs/mom/DESIGN.md`、`DES_CLOSED_LOOP.md`、`IMPLEMENTATION.md`、`TEST_PLAN.md`、`RESULTS.md`、`UI_UX_PRINCIPLES.md`
验证状态：浏览器 + 运行时已覆盖（模拟）；真实系统/模型/权限未验收。
