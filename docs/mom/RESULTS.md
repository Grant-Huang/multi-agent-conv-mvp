# MOM 实现验证记录

## 范围与代码

本次交付 A 层完整模拟流程：独立 SQLite 公告板、事务事件/outbox、DAG、状态化 Mock 工具、计划与资源版本审批、幂等下发、两场景、受限调查 worker、人类和 Agent 双视图。真实工业系统、真实模型调查与企业身份仍未验收，详见 MOCK_REGISTER。可选 LLM Master 适配入口不代表真实模型决策质量已验证。

先基于发布版本实施，后合入远端 `10c86d9` 的最新主干；仅计划文档追加区冲突，保留双方内容。未修改原有 orchestrator/store/registry、CI 或权限策略。新增服务不改变原 8787 API。

## 测试与 Red → Green 证据

初始 MOM 运行时/HTTP 测试在实现前写入；缺模块导致的编译失败仅作开发步骤记录，不算业务 Red。首批新浏览器用例是在实现后补录的基线，不追称测试先行。

随后有实际业务断言失败并保留原断言修复：

1. 跨案例资源竞争：第二个案例仍能占用同一机床。增加原子跨案例资源检查，`two_cases_cannot_reserve_the_same_machine_resources` 通过。
2. 审批界面：两条 Chromium 用例在查找审批人 supervisor 时失败；后端已成功批准，但页面没有呈现审批记录。仅补界面记录展示，原用例通过。初次 `2 failed / 6 passed`，修复后 `8 passed (10.1s)`。
3. 纯时钟事务：`clock_only_transactions_are_protected_against_stale_writers` 初次 `0 passed / 1 failed`，断言 `b.advance(2).is_err()` 不成立。对没有业务事件但状态变化的事务也推进持久序号，修复后包括此用例的 37 项运行时测试全部通过。

未删改既有验收断言，不通过跳过测试消除失败。初次 HTTP 运行的 sandbox 端口拒绝属于环境失败，不算 Red；通过受支持权限运行后通过。

本机证据：`.tmp/mom/browser-red.log`、`/workspace/.setup/mom-clock-red.log`、`/workspace/.setup/mom-green.log`、`/workspace/.setup/mom-release-test.log`。这些是当前机器文件，不假定随 Git 提交保留。

## 最终执行结果

- `cargo fmt --all -- --check`：退出码 0。
- `CARGO_TARGET_DIR=target cargo test --locked`：267 passed、0 failed、2 ignored；其中 MOM Runtime 37、MOM HTTP 3；真实环境忽略项仍为 2，不算通过。
- `cargo clippy --locked --all-targets -- -D warnings`：退出码 0，无警告。
- `cd web && npm run build && npm test`：构建通过；5 个文件、18 个测试通过。
- `cd web && npm run verify`：地图校验 6 项通过，9 个 feature、8 个浏览器 case 有效；TS/Vite、Rust构建、浏览器类型检查、Chromium 8 项通过。
- `tools/websearch`：322 passed、11 deselected（live）、覆盖率 96.19%；flake8 通过。
- `python3 tools/mom/smoke.py --report .tmp/mom/final-smoke-report.json`：13 个 HTTP 功能断言通过；报告记录运行 ID、提交基线、Mock IDs、模板、时区和局限。

浏览器证据在 `web/test-results/` 与 `web/playwright-report/`，包括两个 MOM 场景截图、JSON/JUnit、服务器日志及临时数据库路径。失败保留 trace；重跑输出可能覆盖旧报告。最终 smoke 使用独立 `.tmp/mom-final-smoke.db`，没有操作真实业务系统。

## 当前缺口与运维限制

- Mock 清单：22 MOCK、1 REAL_UNVERIFIED（真实页面，企业权限未验收），0 REAL_VERIFIED；所有复选框保持未勾选。
- 模板检索是结构化基线；尚未做语义召回、LLM 匹配对照或真实历史覆盖率评测。
- 子任务是状态驱动脚本 worker；真实专业模型、现场实验、业务签核仍是替换目标。
- SLA 使用持久 deadline 与虚拟时钟；不具备生产墙钟调度或高可用能力。
- 本机测试身份、资源与动作模拟只能证明控制链行为，不能证明实际质量、安全、诊断或排产正确。
- 测试服务器只清理本次启动的实例；临时数据库和报告保留。既有测试产生的 `crates/server/workspace/files/reports/r.md` 移到 `.tmp/mom/legacy-regression-output/`，避免误提交产物。

## 最新主干合并后的复验

后续主干推进至 `89fcd54`，包含模板运行时、审查返工测试和走马灯用例。已保留所有双方新增用例和功能地图；冲突只涉及追加文档/JSON索引，无源码冲突。复验结果：Rust **303 passed、0 failed、2 ignored**；严格 Clippy 无警告、cargo fmt 通过；前端 **18 passed**；完整 verify **9 passed (12.5s)**，其中 MOM 两条与新增走马灯用例均通过。原先 267/8 的数字保留为此前基线，不作为最终合并版本结果。

## 可配置持续仿真与主界面入口（2026-11-06）

只读核对远端 `2a47f28`，确认 `259e9be` 已在 master；原实现没有主界面菜单，只有独立 8790 服务。本次补上主服务的同源 `/mom/` 和左侧入口，不再让用户在主聊天页寻找不存在的按钮。模拟路由仅在 loopback 主服务挂载，公开监听明确拒绝。

新增：可配置数量/产能/seed、持续质量事件、定时资源与故障变化、持久事件队列、有限自动播放、暂停与重启恢复；gap API 从单一台账生成23项详情。本次没有把任一 Mock 标为真实替换完成。

测试先行证据：新增 HTTP 端点用例初次得到 `404 != 200`，实现后通过。取消用例发现尚未到期事件仍为 PENDING，修复为取消时同步标记所有未来事件；断言未放宽。浏览器检查发现主界面嵌套路由注册冲突和尾斜杠回退问题，修复后保留用例通过。界面加载时保留视图切换能力，原有交互测试仍通过。

最终验证：Rust **311 passed、0 failed、2 ignored**（MOM runtime43、HTTP4）；严格 Clippy 无警告，fmt通过；前端18项通过；完整 verify **12 passed (19.5s)**，包含主界面入口、配置影响排产结果、持续事件与播放暂停。日志位于 `/workspace/.setup/mom-dynamic-*.log`，截图与JSON/JUnit位于 `web/test-results/`。没有进行真实模型、企业接口或墙钟工业调度验收。

合并前主干新增 `4b3f395` 的 MOM 安全修复，已以其保护逻辑为基准整合动态队列；保留全部 rt_mom_01~05 与 HTTP 联锁断言。最终复验：Rust **317 passed、0 failed、2 ignored**（MOM Runtime48、HTTP5）；严格 Clippy、格式通过；Chromium **12 passed (18.3s)**。前端仍为18项通过。日志前缀 `mom-dynamic-merged-*`，先前311项为前一基线。

最后一次推送前主干新增连接状态功能 `4fa1627`，已无冲突合入并复验：Rust **326 passed、0 failed、2 ignored**，Clippy无警告；前端20项通过，完整浏览器12项通过（19.2s）。主界面连接状态与 MOM 入口同时保留。本节为最终合并代码基线，前文数字保留为分阶段证据。
