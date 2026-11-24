# T-91 第 4 线验收：T-88 欠账清零（FEEDBACK 收尾）+ 新壳全量回归 + MOBILE 转绿

| 项目 | 内容 |
|---|---|
| 任务 | T-91 第 4 线验收（任务卡未给 ID，此为建议值，**待用户确认**；T-90 号已被入口适配分支占用） |
| 主线 | **第 4 线：执行协议与共享契约**（[五线分工](../../decisions/2026-11-22-five-workstreams.md)） |
| 授权来源 | 用户第 4 线角色任务卡；#270/#274（T-88 首页与旧壳整治）合并后的本线收尾：FEEDBACK 欠账适配 + 全量回归 + 三层证据 |
| 验收人 | 本 worker；本卡含 **E2E-FEEDBACK 适配**（本线自有用例，非他线实现） |
| 基线 | `origin/master` `fa9970d`（含 #274——T-88 新壳与本线 restart/CODEFLOW 适配、#273 convmem S3、#272/#271） |
| 检出方式 | 隔离 worktree `/tmp/t79-wt`（主工作树为其他卡片分支；本卡未触碰主树文件与未跟踪文件） |
| 日期 | 2026-11-24（UTC） |
| 口径 | 合同 / Playwright harness = fixture·受控模型·进程内服务层；远程真站点不在本轮范围 |
| 运行端口 | harness 18787/18789 被并行会话占用 → 本地错峰 18792/18794（断言未改、补丁已还原）；E2E-RESTART 自管后端 18793；**每轮跑前清本线端口残留**（见 §1.3 坑） |
| G15 / nightly | 11-24 20:45Z（run 45484039389）**failure**——当时 master 尚带 31 例适配欠账与本卡 FEEDBACK 收尾前状态；**Q-T65-01（E2E-MOBILE）本轮确认转绿**，nightly 唯一已知阻塞消失，「连续三晚通过」可从下一个绿夜起算 |
| 证据等级 | 本地所列为 T2；PR CI 为 VERIFIED；master push 红为 **CI flake**（见 §4，与本卡改动无关） |

## 0. 范围依据

#270/#274（T-88）合并后，第 4 线三例适配欠账中 RESTART/CODEFLOW 已随 #274 进主线，**FEEDBACK 为最后一件**。本卡：① 收尾 FEEDBACK 适配；② 新壳全量回归；③ 记录 master rust 红的定位与归属。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| **E2E-FEEDBACK（T-88 最后欠账）** | **VERIFIED / T2** | 适配收尾：新壳右栏以事件流呈现，点开「任务结束」头选中任务后 TaskPanel 渲染任务卡（`.task-card` 在面板非会话内联）；单跑 **2.7s passed**，codeflow 套件 2/2，全套 35 中通过 |
| 持久受理 / 重试去重 / 可查分流 / WP7 A+C / WP2（4 套件合同） | **VERIFIED / T2** | 合同 **25 passed**（accept 4 + replay 5 + restart_semantics 11 + review_verdict 5） |
| 浏览器全组（desktop 31 + restart 1 + codeflow 2） | **VERIFIED / T2** | **35 passed (40.2s)**（干净端口条件下；model.jsonl 50 次调用佐证全程真实 LLM 交互） |
| **E2E-MOBILE** | **转绿（Q-T65-01 收口候选）** | REV+MOBILE **2 passed (2.3s)**——窄屏发送缺陷随 T-88 旧壳整治消失；连续多轮绿后可关闭挂账 |
| E2E-REV / web 单测 / 功能地图 | **VERIFIED / T2** | REV 1 passed；vitest 31/31；test:map **33 features / 93 browser cases valid** |
| PR CI #270～#274 | **VERIFIED** | 已合并全绿（#271/#272 仅 shadow-review SKIPPED，无害） |
| master push | **RED（CI flake，非代码回归）** | `fa9970d`/`b09a949` rust 作业失败 = `mcp.rs::expired_http_session_does_not_replay_tool_calls`；本地同测 **3× passed**（见 §3 归属） |
| nightly | failure（11-24） | 末次失败时点早于本卡收尾与 #274 合并；修复已具备，待下一夜起算 |

**总体**：T-88 第 4 线欠账清零；新壳全量回归绿；**Q-T65-01 转绿**；master rust 红为 CI flake（归属 mcp/agents 测试稳定性，移交实现会话），非代码回归。

---

## 1. 本地证据（worktree @ `fa9970d` + 本卡 FEEDBACK 适配，binary 与 dist 全新构建）

### 1.1 FEEDBACK 适配收尾（本卡代码改动 = `web/e2e/codeflow.spec.ts` 一处导航）

新壳右栏（任务与状态）改为事件流呈现：完成的任务默认折叠为「任务结束」头；点击后 TaskPanel 选中并渲染任务卡。适配 = 点开该头后以**非限定** `.task-card` 定位（卡在右侧面板，不在会话流内联——初版限定 `main` 曾失败）。断言（派错了 → 选 respond → 保存 → 「已记下」）全部不变。

```text
$ npx playwright test e2e/codeflow.spec.ts -g 'FEEDBACK'   → 1 passed (2.7s)
$ npx playwright test e2e/codeflow.spec.ts                 → 2 passed (2.9s)（CODEFLOW+FEEDBACK 顺序依赖复现后修复）
```

顺序依赖定位：FEEDBACK 在 CODEFLOW 之后运行时 `ensureClaudeConnected` 走「已接入」早退，跳过设置面板关闭与发现卡点选；设置对话框残留在右侧面板上方遮住任务卡。修复 = 去掉早退（任何路径收尾都 Esc 关面板）+ 发现卡进入即点掉（知道了）。**断言无放松。**

### 1.2 全量回归

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_accept --test desktop_replay \
    --test restart_semantics --test review_verdict   → 25 passed / 0 failed
$ npx playwright test e2e/desktop.spec.ts e2e/restart.spec.ts e2e/codeflow.spec.ts
  35 passed (40.2s)   ← desktop 31 + RESTART 1 + CODEFLOW 2（含 FEEDBACK）
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE'
  2 passed (2.3s)     ← E2E-REV 1 + E2E-MOBILE 1（转绿，见 §2）
$ npm test            vitest 10 files / 31 tests passed
$ npm run test:map    Feature map valid: 33 features, 93 browser cases
```

harness `model.jsonl` 50 次调用——全程真实 LLM 交互（非陈旧服务应答）。

### 1.3 坑（本轮实证，已写入通知记忆）

**错峰端口孤儿服务污染**：并行会话时代，18792+ 上的孤儿 harness 会让后续运行静默打到死模型（POST 200、零 LLM 调用、直接回答）——本轮 CODEFLOW/FEEDBACK 曾在干净 master 上被如此误判为回归，清本线端口后 2.9s 全绿。**每轮跑前清本线错峰端口 + 核对 `model.jsonl` 增长**；不得清理他人端口。

### 1.4 E2E-MOBILE 转绿（Q-T65-01 收口候选）

REV+MOBILE 2 passed（2.3s）。窄屏「发送」缺陷随 T-88 旧壳整治消失（旧壳顶栏/抽屉结构即缺陷载体）。**候选转正条件**：master 上连续多轮绿 + 今晚起 nightly 通过（T-65/T-66/T-80/T-83 各轮挂账记录随之收口）。

### 1.5 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理回执 / 幂等 / 409 / 丢回执回读 | 通过 | rt_adaccept_01～04、E2E-HOMEACCEPT/ADACCEPT |
| SSE 重放 / 去重 / 410 / 重启对账 / 不串会话 | 通过 | rt_adreplay_01～05、E2E-ADRESUME |
| WP7 A+C 全语义（原因事件/失败交付/重排/开关/家族排除） | 通过 | rt_restart_01～11、E2E-RESTART |
| 判定双出口 / 终态回传 / **派错了反馈（新壳路径）** | 通过 | rt_rev_01～05、E2E-REV/HOMEREV/HOMEREJ/FEEDBACK |
| Mock / 受控模型可辨 | 通过 | harness 受控 LLM（model.jsonl 50 调用）；自管 mock 同口径 |
| 可追踪性 | 通过 | 会话/任务/事件 id 贯通；重排/中断/派错均有留痕 |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #270 | T-88 首页与旧壳整治（第 1 线；评审意见已核对处理） | 已合并（内容经 #274 进主线，见下） |
| #274 | **本线 T-90 入口适配（restart/CODEFLOW + feature-map 冲突解决）** | pass（8/8，已合并） |
| #273 | convmem S3（第 5 线） | pass（8/8，已合并） |
| #271/#272 | docs | pass（仅 shadow-review SKIPPED，已合并） |

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `fa9970d`（#274）/ `b09a949`（#273） | **rust 作业 failure** = `an-agents mcp.rs::expired_http_session_does_not_replay_tool_calls`（mcp.rs:350，1.52s 内失败）。**本地同测 3× passed**（0.01～0.02s）→ CI runner 时序 flake，非代码回归（#273 只动 runtime，an-agents 不依赖之）。归属：mcp/agents 测试稳定性（移交实现会话修复，如加重试/放宽会话过期时序断言） |
| nightly 11-24 20:45Z（run 45484039389） | failure——运行时点早于 #274 合并与本卡 FEEDBACK 收尾；修复均已具备 |
| G15 | 五连败记录在案；**Q-T65-01 已转绿**，下一夜起「连续三晚通过」可重新起算（前提：master push 转绿 + FEEDBACK 收尾合入） |

## 4. 问题清单

| ID | 内容 | 归属 | 状态 |
|---|---|---|---|
| Q-T91-01 | CI flake：`mcp.rs::expired_http_session_does_not_replay_tool_calls`（master push 两连红） | mcp/agents 实现会话（测试稳定性） | 本地 3× 绿；移交 |
| Q-T65-01 | E2E-MOBILE 窄屏发送 | 桌面/前端 | **转绿**（本轮 2.3s；待 nightly 连绿后正式收口） |
| Q-T89-01 | restartServer 端口交接竞态 | 本线测试基建 | 已加固（#268）；根因表述修正为「端口被占用（陈旧监听或 TIME_WAIT）」 |
| — | 错峰端口孤儿服务污染（§1.3） | 全线纪律 | 已写入通知记忆与 #270 留言 |

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 新壳（T-88）下第 4 线全链路回归绿：受理→编排→判定→终态回传→重启双语义→**派错反馈** | 断点续跑不做（决策维持）；模板家族级恢复另卡 |
| **E2E-MOBILE 转绿**，nightly 阻塞清空 | master rust 红（Q-T91-01 CI flake）待实现会话修复；G15 三晚重算待起 |
| FEEDBACK 适配收尾，T-88 第 4 线欠账清零 | 模板家族验收面（rt_restart_11）随家族级恢复另卡 |

## 6. 非目标（本卡遵守）

- 业务代码零改动（唯一代码改动 = FEEDBACK 导航适配的测试文件）；断言无放松
- 不对 #249/#264/#274 之外的他线内容越界处置（mcp flake 仅定位与移交）
- 未跑远程真站点 live；端口纪律（只清本线 18792/18793/18794，他人端口不碰）
