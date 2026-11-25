# T-94 第 4 线验收：R-20261124 修复三卡合并后回归 + H3 fix_claimed 登记

| 项目 | 内容 |
|---|---|
| 任务 | T-94 第 4 线验收（派工预留号） |
| 主线 | **第 4 线：执行协议与共享契约**（[五线分工](../../decisions/2026-11-22-five-workstreams.md)） |
| 授权来源 | 用户第 4 线角色任务卡；R-20261124 修复三卡（#285/#286）合并后的本线收尾：全量回归 + `findings.jsonl` H3 `fix_claimed` 登记（方案规定的合并后动作） |
| 验收人 | 本 worker；**#285（T-93）作者=本会话，其修复效果验收不在本卡宣称**（宪章原则 4）——本卡做回归复跑；`verified_fixed` 由下一次每晚审查核实（方案流程） |
| 基线 | `origin/master` `467d6e0`（含 #285 T-93 H3 修复、#286 T-95 FIX-C 夹具门控与事件去重、#279～#284） |
| 检出方式 | 隔离 worktree `/tmp/t79-wt`（主工作树为其他卡片分支；本卡未触碰主树文件与未跟踪文件） |
| 日期 | 2026-11-25（UTC） |
| 口径 | 合同 / Playwright harness = fixture·受控模型·进程内服务层；远程真站点不在本轮范围 |
| 运行端口 | harness 18787/18789 被并行会话占用 → 本地错峰 18792/18794（断言未改、补丁已还原）；E2E-RESTART 自管后端 18793；跑前清本线端口残留 |
| G15 / nightly | 11-24（run 45484039389）failure 后，11-25 定时截至收稿**未触发**。关键变化：**E2E-MOBILE 与 E2E-FEEDBACK 均已在 master 转绿**（本轮 2.3s / 43.2s 复跑证实）——nightly 已知阻塞清空，下一夜起「连续三晚通过」可起算 |
| 证据等级 | 本地所列为 T2；PR CI 为 VERIFIED；master push @ `467d6e0` **success**（run 45519350458） |

## 0. 范围依据

R-20261124 修复三卡：FIX-A（T-92，WA 线）、**FIX-B（T-93，本线，#285 已合并）**、FIX-C（T-95，桌面线，#286 已合并）。本卡 = 合并后全量回归（重点：FIX-C 改动 `store.rs write_events` 事件去重与 `desktop_api.rs` 夹具门控，与本线重放/重启链有交互面）+ H3 `fix_claimed` 登记。

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| 4 套件合同（accept/replay/restart_semantics/review_verdict） | **VERIFIED / T2** | **25 passed**——FIX-C 的 `write_events` 去重（E2）未破坏 SSE 重放/去重/重启对账语义（rt_adreplay_01～05 与 rt_restart_01～11 全绿） |
| 浏览器全组（desktop 31 + restart 1 + codeflow 2） | **VERIFIED / T2** | **35 passed (43.2s)**；`model.jsonl` 50 调用佐证非污染 |
| E2E-FEEDBACK（T-88 最后欠账） | **VERIFIED / T2** | master 上通过（适配随 #277 进主线） |
| E2E-MOBILE | **转绿（第 3 轮连续）** | 2.3s passed——Q-T65-01 收口候选转正条件持续满足 |
| E2E-REV / web 单测 / 功能地图 | **VERIFIED / T2** | REV 1 passed；vitest 31/31；test:map 33 features / 93 browser cases valid |
| H3 `fix_claimed` 登记 | **完成** | `findings.jsonl` 追加事件（pr #285、commit `c2eba56`、test RT-TPL-17）；`verified_fixed` 待下晚审查 |
| PR CI #275～#286 | **VERIFIED** | 已合并者全绿（#276 仅 shadow-review SKIPPED） |
| master push @ `467d6e0` | **success** | run 45519350458 |
| nightly | 11-25 未触发 | G15 待今晚起算（阻塞已清空） |

**总体**：修复三卡合并后第 4 线全链路回归 **PASS**，无新增缺陷；E2E-MOBILE/FEEDBACK 双转绿使 nightly 具备转绿条件。

---

## 1. 本地证据（worktree @ `467d6e0`，binary 与 dist 全新构建）

### 1.1 合同层（4 套件 25/25）

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server \
    --test desktop_accept --test desktop_replay \
    --test restart_semantics --test review_verdict
desktop_accept:      4 passed    desktop_replay:     5 passed
restart_semantics:  11 passed    review_verdict:     5 passed
合计 25 passed / 0 failed
```

### 1.2 浏览器与 web

```text
$ npx playwright test e2e/desktop.spec.ts e2e/restart.spec.ts e2e/codeflow.spec.ts
  35 passed (43.2s)
$ npx playwright test e2e/features.spec.ts -g 'REV|MOBILE'
  2 passed (2.3s)        ← E2E-REV + E2E-MOBILE（转绿第 3 轮）
$ npm test               vitest 10 files / 31 tests passed
$ npm run test:map       Feature map valid: 33 features, 93 browser cases
```

harness `model.jsonl` 50 次调用——全程真实 LLM 交互。

### 1.3 H3 `fix_claimed` 登记（方案规定的合并后动作）

```text
$ tail -1 docs/review/findings.jsonl
{"id": "R-20261124-H3", "at": "2026-11-24", "by": "fixer", "event": "fix_claimed",
 "pr": ".../pull/285", "commit": "c2eba568f920…", 
 "tests": ["rt_tpl_17_join_created_once_under_concurrent_completion"]}
```

### 1.4 核对清单（副作用 / 回执 / 版本绑定 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 受理回执 / 幂等 / 409 / 丢回执回读 | 通过 | rt_adaccept_01～04、E2E-HOMEACCEPT/ADACCEPT |
| SSE 重放 / 去重 / 410 / 重启对账 / 不串会话（**FIX-C 事件去重后**） | 通过 | rt_adreplay_01～05（重复事件去重语义与 E2 对齐）、E2E-ADRESUME |
| WP7 A+C 全语义（原因事件/失败交付/重排/开关/家族排除） | 通过 | rt_restart_01～11、E2E-RESTART |
| 汇合只建一次（**T-93 修复后回归**） | 通过 | rt_tpl_17 在 workspace 全量中通过；E2E 模板链路无重复交付 |
| 判定双出口 / 终态回传 / 派错反馈 | 通过 | rt_rev_01～05、E2E-REV/HOMEREV/HOMEREJ/FEEDBACK |
| Mock / 受控模型可辨（**FIX-C 夹具门控后**） | 通过 | harness 配置打开 `test_fixtures`（E2E 环境），生产缺省 false——门控行为由 #286 的 `desktop_fixture_gate` 套件覆盖 |
| 可追踪性 | 通过 | 会话/任务/事件 id 贯通；H3 fix_claimed 留痕 findings.jsonl |

---

## 2. PR CI 证据

| PR | 内容 | 结果 |
|---|---|---|
| #276 | H1-H4 修复方案（决策文档） | pass（仅 shadow-review SKIPPED，已合并） |
| #277 | **本线 T-91（FEEDBACK 收尾 + MOBILE 转绿）** | pass（8/8，已合并） |
| #278/#279/#280/#282/#283/#284 | 各线验收/适配/证据 | pass（8/8，已合并） |
| #285 | **本线 T-93（H3 汇合只建一次）** | pass（8/8，已合并 `9cd3e08`） |
| #286 | T-95 FIX-C 夹具门控与事件去重（桌面线） | pass（8/8，已合并 `467d6e0`） |

## 3. master push / 定时证据

| 项 | 状态 |
|---|---|
| push @ `467d6e0`（#286） | **success**（run 45519350458） |
| push @ `9cd3e08`（#285） | cancelled（新 push 排队取代，非失败） |
| nightly | 11-25 截至收稿未触发；11-24 failure 时点早于全部修复具备；G15 起算条件已就绪（MOBILE/FEEDBACK 转绿 + 适配全合入） |

## 4. 问题清单

| ID | 内容 | 归属 | 状态 |
|---|---|---|---|
| Q-T91-01 | `mcp.rs::expired_http_session_does_not_replay_tool_calls` CI flake | mcp/agents 实现会话 | 移交中（本轮未复发） |
| 坑 7 | `rt_adm_14` 本机特有失败 | 历史遗留 | 不变 |
| — | **无新增缺陷** | — | 修复三卡合并后全链路绿 |

## 5. 相对目标的差距（主轴对照）

| 已达成（本轮核实） | 仍差距 |
|---|---|
| 受理→编排→汇合（**只建一次**）→判定→终态回传→重启双语义→派错反馈全链路绿 | 断点续跑不做（决策维持）；模板家族级恢复另卡 |
| **E2E-MOBILE / E2E-FEEDBACK 双转绿**，nightly 阻塞清空 | G15 三晚连过待今晚起算；mcp.rs CI flake 待实现会话修复 |
| 合同 25/25 + 浏览器 35+2 + 地图 93 cases + fix_claimed 留痕 | 远程真站点 live（Q-T60-02，非本轮目标） |

## 6. 非目标（本卡遵守）

- 业务代码零改动（唯一非文档改动 = `findings.jsonl` 一行 `fix_claimed`，方案规定的合并后登记动作）
- 不对 #285（T-93，作者=本会话）宣称独立验收；`verified_fixed` 归下晚审查
- 未跑远程真站点 live；端口纪律（只清本线 18792/18793/18794）
