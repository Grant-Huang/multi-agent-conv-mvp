## 变更说明
<!-- 解决什么问题、改了什么、为什么这样改。 -->

## 关联
- Case / Feature ID：<!-- 如 E2E-APPROVE、RT-FENCE-01、Feature ID：approval；无则写「基线补录」并说明 -->
- 需求 / Bug / 审查编号：

## 设计一致性
<!-- 见 docs/DESIGN.md 第 0.3 节、docs/contracts/、docs/PRODUCT.md。后两项答“是”时须先有方案并获批准。 -->
- 属于哪个特性（DESIGN 第 0.3 节）：
- 是否触及共享契约（docs/contracts/）：否 / 是（哪一份、哪一条）
- 是否偏离产品宪章（docs/PRODUCT.md）：否 / 是（须先有用户确认的决策）
- 新增或修改的决策（docs/decisions/）：无 / 文件名

## 非目标
<!-- 本次明确不改什么。防止 AI 顺手扩大范围。至少写清：不改哪些模块 / 不改哪些 API。 -->
-

## 风险与回滚
- 风险等级（以 CI `change-policy` 输出为准）：
- 是否涉及并发 / 子进程 / 权限 / 敏感数据 / 状态机 / 持久化：
- 回滚方式（含数据库迁移如何处理）：

## 验证证据
<!-- 贴实际执行的命令和结果行，不要只写"已通过"。 -->
- [ ] `CARGO_TARGET_DIR=target cargo test --locked`
- [ ] `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets`
- [ ] `cd web && npm run build`
- [ ] 修 bug 附回归测试；新行为有 Red→Green 证据
- [ ] （若改功能地图）`cd web && npm run test:map`

## 未覆盖的测试
<!-- 没测的部分如实写。 -->

## 人工确认
- [ ] 改动符合需求，证据真实；作者不能算作独立审批人
- [ ] 涉及子进程 / 异步时，已说明超时、取消和清理策略
- [ ] 涉及外部内容进入 Agent 上下文时，已按 7-fence-review.md 第 4 节检查
- [ ] L2/L3 路径已按 AGENTS.md「先停下来问人」给出方案或获得批准

<!--
CI 会读取 PR 描述中的下列二级标题，仅在确有需要时才新增（本模板不预置，避免空占位被当成说明）：
大改动说明（≥ 40 字）、测试删除说明（≥ 20 字）、测试变更说明（≥ 20 字）、注入扫描说明（≥ 20 字）。
非文档改动缺少「非目标」或 Case/Feature ID 时，change-policy 会警告。
-->
