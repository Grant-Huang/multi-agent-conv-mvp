# World Adapter 与 MOM 工作台 Feature Map

Backed by：`docs/mom/WORLD_ADAPTER.md`、`docs/mom/WORLD_ADAPTER_PLAN.md`、`docs/mom/DES_EXPERIMENT_OPS_SPLIT.md`、`docs/mom/WA_MOCK_REGISTRY.md`、`web/e2e/des-experiment.spec.ts`、`web/e2e/mom-workbench.spec.ts`、`web/e2e/wa-integration.spec.ts`、`scripts/wa-m0-suite.sh`、`web/src/desktop/DesExperiment.tsx`、`web/src/desktop/MomWorkbench.tsx`、`web/src/components/OpsCards.tsx`；用例 `E2E-DESTRIAL`～`E2E-DESLOOP`（M0-3/M0-5 双上下文）、`E2E-WABIND`（WA-01）、`E2E-WACHANNEL`（WA-05 适用切片）；合同 `RT-WA-01～09`；准出证据 `docs/testing/evidence/wa-m0-6.md`。  
现有路径见 [MOM Feature Map](mom.md)。表中其余 `E2E-WA-*` 为设计编号，登记进 CASES/`features.json` 前须换成 `E2E-[A-Z]+`；控件名是 Playwright 定位契约。M0 H4 覆盖矩阵见 [WA_MOCK_REGISTRY.md](../../mom/WA_MOCK_REGISTRY.md) §4。M0 已准出（#175）；**M1** 正式 id `E2E-WADES` / `WAREAD` / `WAOBS`、`RT-WADES-01`～`08` 已登记（`wa-des-remote.spec.ts`、`mom_world_des.rs`），见 [WA_M1_TASKS.md](../../mom/WA_M1_TASKS.md)。

| 拟用例 | 正式 E2E id | 操作路径与定位 | UI 和业务断言 | 追溯 |
|---|---|---|---|---|
| E2E-WA-01 世界绑定 | `E2E-WABIND`（M0 Mock）；`E2E-WADES`（M1 远程，Red） | 「MOM 工作台」→「选择世界」→「看板」「任务」「资源」分别打开同一事项 | 同一 world/case；返回保持世界与入口；跨世界列表不泄露 | WA-01、08 / UI01、02、16 / P-MOM-ENTRY、SCOPE、RETURN |
| E2E-WA-02 续读 | `E2E-WAREAD`（M1，Red） | 「看板」断线，重连或点「重新读取」 | 从提交边界补事件，同 ID 只应用一次；过期和水位单独显示 | WA-02、07 / UI03、15 |
| E2E-WA-03 条件 | （待登记） | 「任务」→「受阻」→「刀具调拨」，从「资源」→「料」再次定位 | 受理后待实物交接，首件未通过不能放行；冲突引用同一资源并无重复预留 | WA-08 / UI03、05、06、11 / P-MOM-STATE、VIEWS |
| E2E-WA-04 节点和来源 | （待登记） | 「处理进展」→「维护初筛」→「证据与记录」「通俗解释」，打开「流程选择依据」 | 一次点击可见结论，候选假设仍待验证，草稿流程不可执行，证据来源可查 | WA-07、08 / UI04、07、08、15 |
| E2E-WA-05 审批 | `E2E-WACHANNEL`（入口切片；冲突/旧授权见后续） | 「方案与决定」→「前往运营频道」→同案例卡「批准」；模拟同时修改版本 | 工作台可进运营频道；完整冲突断言随 M1+ | WA-03 / UI07、09 |
| E2E-WA-06 恢复 | （待登记） | 「执行已批准动作」→「查看执行」→「查证结果」 | 重试只执行一次；部分成功可区分；取消后无额外写，迟到回执不改变终态 | WA-04 / UI10、15 |
| E2E-WA-07 动态 DES | `E2E-WAOBS`（M1 观测切片，Red）；完整干预闭环随 M2 | 会话进入动态 DES → 推进 → 观测/事项 | M1：正式投递观测可见且无标准答案；M2：批准维修后未来事件改变 | WA-05、07、08 / UI07、14、15 / RT-DES-08～11、RT-DESC |
| E2E-WA-08 会话和小屏 | （待登记） | 「与 AI 讨论」写 A 草稿 → 切 C → 回 A；节点证据页按 Escape | 草稿和上下文隔离，焦点恢复，小屏无横溢，模型离线时已有证据可查 | WA-07、08 / UI12、16 / P-MOM-CONTEXT |
| E2E-WA-09 客诉与移交 | （待登记） | C101 →「承诺与交接」→检查冻结、回复、8D 与接责 | 缺依据不能关闭。4 小时回复、24 小时报告与关闭各自验证。移交保留风险 | WA-08 / UI13、14 |
| E2E-WA-10 生产确认 | （待登记） | 选择生产世界 → 资产来源及水位 →「创建维修工单」→批准→「查证结果」 | 按工单号核对站点、设备与状态。延迟显示待查证。工单记录和设备恢复分开 | WA-06、07 / UI02、09、10、15 |

Playwright 先用 Harness 装载场景和时钟，再从产品首页开始。以拥有明确名称的导航、事项区、对话框或频道卡限定定位范围。重复业务动作使用案例或执行 ID 定位，例如在对应 OpsCase 内定位角色 button 和精确名称「批准」。故障准备可以调用测试入口，正常操作经 UI。通过正常业务 API 核对副作用和版本。

正式验证矩阵随实施登记到 features.json、CASES.md、COVERAGE.md。各用例绑定其适用的 Mock、远程 DES 和站点环境。原型证据见 [浏览器运行记录](../../mom/prototype/TEST_RESULTS.md)，协议目标用例当前待实现。

## DES 实验与运营入口拆分

目标入口为「设置与管理 → DES 仿真实验」和「AI 桌面 → 运营频道」。实验员、运营员、审核员使用独立浏览器上下文。用有界测试时钟准备场景，在真实前端执行操作，从业务接口读取事件、水位、事项、卡片和回执验证效果。控件名是设计定位契约。

| 设计编号 | 正式 E2E id | 用户路径 | 业务与 UI 断言 | 适配器 |
|---|---|---|---|---|
| DES-UI-01 | `E2E-DESTRIAL` | 实验员打开「配置」，保存，再点「试跑」 | 非法参数明确提示；预览有试跑 ID，运营频道新增卡为零 | M0 Mock，M1 DES |
| DES-UI-02 | `E2E-DESSTART` | 核对配置版本和目标频道，点「启动正式运行」，运营员打开频道 | 重试仍只有一个正式运行；试跑与正式运行 ID 不同；同一事项只发一张正确标注的卡 | M0 Mock，M1 DES |
| DES-UI-03 | `E2E-DESOPS` | 运营员打开异常卡 →「查看方案」→「批准」→「查看执行」 | 完全在 AI 桌面处置。审批匹配方案、事项和世界。执行证据更新原卡 | M0 Mock，M2 DES |
| DES-UI-04 | `E2E-DESPAUSE` | 实验员「暂停」；运营员看等待状态；实验员「单步推进」；运营员重连 | 等待状态区别于失败。推进后回执正确。重复及迟到事件不复制或回退卡片 | M0 Mock，M1 覆盖读和卡片，M2 全量 |
| DES-UI-05 | `E2E-DESISO` | 运营员尝试进入实验控制；用运行 A 的审批提交到 B | 服务端拒绝、列表和详细数据无泄露，启动或业务动作计数不变 | M0 Mock，M1 实验权限，M2 执行范围 |
| DES-UI-06 | `E2E-DESLOOP` | 实验员启动正式运行；运营员批准维修；实验员推进；运营员查看核验证据 | 已投递事件保留，未来事件随干预更新。两端可关联到同一执行，标准答案只对审核权限可见 | M0 业务路径 Mock，M2 物理断言 DES |

控件名契约：`DES 仿真实验`、`配置`、`试跑`、`启动正式运行`、`暂停`、`单步推进`、`运营频道`、`查看方案`、`批准`、`查看执行`、`查看核验证据`；测试 id：`trial-run-id`、`official-run-id`、`ops-delivery-count`、`ops-case-card`、`execution-receipt`、`verify-evidence`。  
DES-UI 场景补充 WA-01、04、05、07、08；原有 WA 和 UI 回归随里程碑运行。Mock 通过、真实 DES 通过分别登记。
