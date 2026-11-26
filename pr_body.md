[第 3 线] 发布器分级不再用消费时刻阶段回溯：DecisionRequested 按种类固定 P0（#384）

## 变更说明
Issue #384（高）：`OpsPublisher.consume()` 对每条事件用案例**消费时刻**的阶段调 `classify`——新案例在首个发布节拍前已被批准并执行（stage 到 `VERIFYING`）时，其创建期 `DecisionRequested` 事件被降级 P1、命中 `p1_no_card`，**P0 案例永远不出卡**（对照：先等卡发布再执行则正常出卡）。

修复一行语义：`DecisionRequested` 加入 `P0_KINDS`。决策请求事件本身就是「等待人决定」的发生时刻事实，按**事件种类**固定 P0，与消费时刻的案例阶段无关。`classify` 仍是纯函数、同输入同输出（RT-OPSDESK-01 语义不变）。

### 大改动说明
本 PR 共 84 行：1 处常量数组加一项（含注释）、1 个新 RT 用例（33 行）、CASES.md/features.json 登记与证据文件。行为变化仅覆盖「案例在首个发布节拍前已被推进到后置阶段」这一缺陷类别；待批准案例出卡路径原本就经 `needs_human` 得 P0，不受影响。

## 关联
- Case / Feature ID：RT-OPSDESK-14；Feature ID：ops-desk
- 需求 / Bug / 审查编号：Issue #384（第 1 线写 #341 E2E 时发现，按 #312 派给工业业务闭环线）

## 设计一致性
- 属于哪个特性（DESIGN 第 0.3 节）：运营桌面与运营频道（ops-desk，`docs/mom/OPS_DESK.md` §5 事件分级——P0「案例待批准」本就该发卡）
- 是否触及共享契约（docs/contracts/）：否
- 是否偏离产品宪章（docs/PRODUCT.md）：否
- 新增或修改的决策（docs/decisions/）：无（修复方向即 issue 建议方向之一，分级仍集中在一个纯函数）

## 非目标
- 不改发布器的游标/合并窗口/小时上限/汇总卡机制（RT-OPSDESK-02/05 验证不回退）。
- 不改引擎事件结构（不给事件加阶段字段——本修复不需要）。
- 不改 `classify` 的其他输入语义；`p1_no_card` 分支保留（真正的 P1 无卡案例仍不新发）。

## 风险与回滚
- 风险等级（以 CI `change-policy` 输出为准）：L2（默认等级；本地 `risk_classify.py` 输出 L2）
- 是否涉及并发 / 子进程 / 权限 / 敏感数据 / 状态机 / 持久化：否（发布器消费侧一个分级常量；无状态/持久化变化）
- 回滚方式：revert 本 PR 即恢复原状；无迁移。

## 验证证据
- [x] Red→Green：`docs/testing/evidence/ops-publisher-p0-t384.md`（Red：`rt_opsdesk_14` FAILED，被执行案例被 `p1_no_card` 吞掉；Green：11 passed）
- [x] `CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test ops_desk`：11 passed / 0 failed
- [x] `CARGO_TARGET_DIR=target cargo test --locked -p an-runtime -p an-server`：除 `rt_adm_14`（已知本地环境既有问题，pristine `origin/master` 冷 worktree 同样失败，与本改动无代码交集）外 25 个测试二进制全绿
- [x] `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings`：退出码 0；`cargo fmt --all -- --check` 干净
- [x] `cd web && npm run typecheck && npm test && npm run build && npm run test:map`：31 passed；`Feature map valid: 33 features, 99 browser cases`
- [x] E2E：`npx playwright test e2e/ops.spec.ts --workers=1` → 7 passed (47.4s)，既有 E2E-OPS* 零回归

## 未覆盖的测试
- 纯浏览器形态的 #384 复现（issue 用 API 序列描述；RT 用例与之同构且确定性更强，浏览器断言由既有 E2E-OPSCHANNEL/OPSNOISE 覆盖）。

## 人工确认
- [x] 改动符合需求，证据真实；作者不能算作独立审批人（L2 待 1 位非作者审查人批准）
- [x] 不涉及子进程 / 异步 / 外部内容进上下文
- [x] L2/L3 路径已按 AGENTS.md「先停下来问人」给出方案或获得批准：不触及高风险清单文件

🤖 Generated with [Claude Code](https://claude.com/claude-code)
