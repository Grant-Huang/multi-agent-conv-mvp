# 三入口独立原型验证

日期：2026-11-08（America/Vancouver）。范围仅为 `docs/mom/prototype/` 的模拟界面，非生产MOM验收。

## 用例先行与Red

先新增 `check-entries.cjs` 的三入口、范围与跨入口状态断言，再改原型。在可访问的旧版原型运行该脚本，实际失败为导航不匹配：

```text
actual:   ['▦我的工作4', '▤车间公告板', '◇流程进展', '▥资源条件', '⇄交接记录']
expected: ['▤看板', '✓任务', '▥资源']
AssertionError [ERR_ASSERTION]
```

这是行为差异，不是连接失败。既有 `check.cjs`、`check-nodes.cjs` 只迁移导航定位到新事项入口及页内标签，保留节点结果、证据、解释、受理边界、审批和会话断言。标签重复造成的测试定位歧义只通过限定 `.worktabs` 解决，未删业务断言。

## Green

三个脚本全部退出0，浏览器无脚本错误：

```text
PASS: three entries, scopes, folds, shared case/state, direct resource action, return context, scoped drafts, mobile
PASS: DAG context, scoped AI, six dimensions, explicit approval, board, handover, Pad and mobile layout
PASS: direct result, evidence, explanation, condition handling, receipt boundary, approval and mobile
```

- 桌面只保留三个入口；与我相关、本班组、本车间分别展示2、1、3个样例事项，范围跨入口保持。
- 已完成任务默认折叠、受阻展开；资源支持全部及六维，未知不显示正常。
- 从看板批准、任务模拟调拨受理，再从资源进入同一事项，批准与受理状态一致；受理后仍待实物交接，不解锁批量生产。
- 返回事项前的范围、维度与滚动位置保留；事项会话及草稿隔离；交接只引用当前事项。
- 节点点击直接看完成结果、证据、解释，或处理条件；首件条件未满足时放行按钮不可用。
- Pad会话可操作；390px看板、任务和资源无主页面横向溢出；小屏节点工作区不超屏。

## 仓库检查

```text
CARGO_TARGET_DIR=target cargo test --locked --offline
  387 passed, 0 failed, 2 ignored（各测试目标汇总）
CARGO_TARGET_DIR=target cargo clippy --locked --offline --all-targets
  Finished dev profile ... in 0.17s（无警告）
cd web && npm run build
  280 modules transformed / built in 175ms
cd web && npm run test:map
  tests 1 / pass 1 / fail 0
  Feature map valid: 15 features, 14 browser cases
```

原型检查使用本地静态服务；页面无网络写入或模型调用。截图由 `capture.cjs` 生成。验证不包括真实工业回执、权限后端、通用DAG执行／汇合、生产实时更新或正式可用性测试；以上仍是产品实施缺口。
