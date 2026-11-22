# T-60 跨组场景独立验收

| 项目 | 内容 |
|---|---|
| 任务 | T-60 跨组场景独立验收 |
| 验收人 | 本 worker（未参与 #198～#202 实现） |
| 基线提交 | `43014db`（`origin/master`，含 #198～#202） |
| 日期 | 2026-11-22（UTC） |
| 口径 | 合同测 / 已入库 Playwright = fixture·Mock·进程内服务层；远程真 `des_mcp` = **NOT_VERIFIED** |
| G15 / nightly | **本轮不纳入** |

## 结论（摘要）

| 链路 | 等级 | 说明 |
|---|---|---|
| Desktop C2：RT-ADAPPR / RT-ADATTN + E2E-ADAPPR / E2E-ADATTN | **VERIFIED / T2** | 本地合同 7/7；桌面入口 Playwright 2/2（需先 `npm run build`） |
| Desktop D：RT-ADVOICE / RT-ADHAND + E2E-ADVOICE / E2E-ADHAND | **VERIFIED / T2**（fixture） | 本地合同 7/7；Playwright 2/2；真机麦/双设备仍 NOT_VERIFIED |
| WA M2 写合同 RT-WADES-09～14 | **VERIFIED / T2** | `mom_world_des_write` 含 09～19 共 11 passed |
| 失败/恢复 RT-WADES-15～17（含 11 取消守卫） | **VERIFIED / T2** | 15～17 ok；`rt_wades_11_cancel_pending_not_dispatched` ok |
| `wa-m2-regression` | **VERIFIED / T2** | write 11 + read 12 + RT-DES 4 + RT-DESC 6 → green |
| 远程真 DES MCP | **NOT_VERIFIED** | `AN_MOM_DES_MCP` unset；未跑 live bind |
| Playwright 写路径 E2E-WAEXEC/WARECV/WADYN/WALOOP | **未宣称** | 仍属 M2-5 |
| master 合入后 push CI（`43014db`） | **VERIFIED** | rust/web/security/python-* success；change-policy skipped（push） |

**总体**：合同层与桌面入口跨组复跑 **PASS**；远程真厂路径与 WA 写路径浏览器闭环 **不宣称 VERIFIED**。

---

## 1. 本地证据（本会话复跑）

环境：Cloud Agent VM；`CARGO_TARGET_DIR=target`；分支检出 `43014db`。

### 1.1 Desktop C2 合同（#198）

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_c2 --locked -- --nocapture
running 7 tests
test rt_adappr_01_approve_matching_revision ... ok
test rt_adappr_02_bump_invalidates_old_execute ... ok
test rt_adappr_03_reject_blocks_execute ... ok
test rt_adappr_04_execute_once ... ok
test rt_adappr_05_unknown_then_ack ... ok
test rt_adattn_01_mute_drops_ordinary_keeps_urgent ... ok
test rt_adattn_02_dedupe_ordinary_by_title ... ok
test result: ok. 7 passed; 0 failed; 0 ignored
```

Mock 标记：store 副作用计数，不写厂（与 `ai-desktop-c2.md` / 决定一致）。

### 1.2 Desktop D 合同（#201，可选）

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_d --locked -- --nocapture
running 7 tests
… rt_advoice_01～04 / rt_adhand_01～03 … ok
test result: ok. 7 passed; 0 failed; 0 ignored
```

### 1.3 WA 写合同 + 失败/恢复（#199/#200/#202）

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test mom_world_des_write --locked -- --nocapture
running 11 tests
test rt_wades_09_propose_binds_plan_revision ... ok
test rt_wades_10_execute_idempotent ... ok
test rt_wades_11_cancel_pending_not_dispatched ... ok
test rt_wades_12_confirm_after_lost_receipt ... ok
test rt_wades_13_factory_instance_change_rejects_stale ... ok
test rt_wades_14_intervention_changes_future_events ... ok
test rt_wades_15_recover_by_correlation_key ... ok
test rt_wades_16_complete_partial_receipts ... ok
test rt_wades_17_factory_restart_history_no_misdispatch ... ok
test rt_wades_18_answer_matches_intervention ... ok
test rt_wades_19_answer_sync_retry_after_failure ... ok
test result: ok. 11 passed; 0 failed; 0 ignored
```

### 1.4 `wa-m2-regression`（#202）

```text
$ ./scripts/wa-m2-regression.sh
== RT-WADES write == 11 passed
== RT-WADES read  == 12 passed
== RT-DES-08～11  == 4 passed
== RT-DESC connector == 6 passed
== wa-m2 regression green ==
```

### 1.5 桌面入口 Playwright（成功 + 可选 D）

前置：本环境初跑时无 `web/dist`，页面空白导致 4 条 timeout；执行 `cd web && npm run build` 后复跑通过（见问题清单 Q-T60-01）。

```text
$ cd web && npx playwright test e2e/desktop.spec.ts -g 'ADAPPR|ADATTN|ADVOICE|ADHAND'
Running 4 tests using 1 worker
✓ E2E-ADATTN
✓ E2E-ADAPPR
✓ E2E-ADVOICE
✓ E2E-ADHAND
4 passed (4.3s)
```

Harness：`e2e/harness.mjs`（`world_mock=true`，推理 Mock，真实 API/SQLite）；入口 `/` → SMT-03 / 会话控件。

---

## 2. PR CI 证据（合入前 head）

| PR | head SHA | rust | web | security | change-policy | 其它 python |
|---|---|---|---|---|---|---|
| [#198](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/198) C2 | `b86e34e` | success | success | success | success | success |
| [#199](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/199) M2-2 | `f5b1e1f` | success | success | success | success | success |
| [#200](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/200) M2-3 | `917bba0` | success | success | success | success | success |
| [#201](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/201) D | `25112c6` | success | success | success | success | success |
| [#202](https://github.com/Grant-Huang/multi-agent-conv-mvp/pull/202) M2-4 | `735c2bd` | success | success | success | success | success |

说明：PR CI **不跑** Playwright 桌面 E2E；合同测随 `cargo test` 覆盖。Playwright 以本会话本地为准。

---

## 3. master 合入后 / 定时

| 项 | 状态 |
|---|---|
| push CI @ `43014db`（merge #202） | rust/web/security/python-* **success**；change-policy **skipped**（push 事件）— run [45000348854](https://github.com/Grant-Huang/multi-agent-conv-mvp/actions/runs/45000348854) |
| G15 / `GR-4-NIGHTLY-E2E` | **本轮不纳入**；未查 nightly |

---

## 4. 核对清单（副作用 / 回执 / 版本 / Mock / 可追踪）

| 检查点 | 结果 | 依据 |
|---|---|---|
| 审批 revision 绑定与升版失效 | 通过 | RT-ADAPPR-02；E2E-ADAPPR |
| 拒绝后不执行；执行恰好一次；unknown→ack | 通过 | RT-ADAPPR-03～05 |
| Attention mute / urgent / 去重 | 通过 | RT-ADATTN-01～02；E2E-ADATTN |
| Propose 绑定 plan_revision | 通过 | RT-WADES-09 |
| Execute 幂等；取消守卫 | 通过 | RT-WADES-10/11 |
| 丢回执 Confirm；关联键恢复 | 通过 | RT-WADES-12/15 |
| 工厂实例变化拒陈旧；重启不误投 | 通过 | RT-WADES-13/17 |
| 部分回执补齐；干预与答案同步 | 通过 | RT-WADES-16/18/19 |
| Mock / fixture 可辨 | 通过 | harness `world_mock`；桌面 fixture 媒体；证据不宣称真厂 |
| 远程真 des_mcp | **NOT_VERIFIED** | 环境无 `AN_MOM_DES_MCP` |

---

## 5. 问题清单（可复现）

### Q-T60-01 — Playwright 缺 `web/dist` 时桌面入口空白

| 字段 | 内容 |
|---|---|
| 提交 | `43014db` |
| Case | E2E-ADAPPR / ADATTN / ADVOICE / ADHAND |
| 步骤 | 未先 `npm run build`；直接 `npx playwright test e2e/desktop.spec.ts -g 'ADAPPR\|…'` |
| 期望 | 桌面 S1 可见 SMT-03 / 新会话 |
| 实际 | 白屏；超时等 `card-smt-03` 或「＋ 新会话」；4 failed |
| 恢复 | `cd web && npm run build` 后同命令 **4 passed** |
| 建议归属 | Desktop / 文档·验收指引（非业务回归）；建议在 FEATURE_MAP 或 testing README 写明「Playwright 前置：web/dist + target/debug/agentnexus」 |
| 严重度 | 环境/指引（实现组勿改断言迎合） |

### Q-T60-02 — 远程真 DES 写/读 live 本轮未核

| 字段 | 内容 |
|---|---|
| 提交 | `43014db` |
| Case | live `RT-WADES-*`（真连接器） |
| 阻塞 | `AN_MOM_DES_MCP` unset |
| 期望 | 有环境时 bind/tick/写路径 live VERIFIED |
| 实际 | **NOT_VERIFIED**；仅桩/进程内服务层 |
| 建议归属 | World Adapter；另开验收卡配 MCP |

### Q-T60-03 — WA 写路径浏览器 E2E 仍属 M2-5

| 字段 | 内容 |
|---|---|
| Case | E2E-WAEXEC / WARECV / WADYN / WALOOP |
| 实际 | 本轮未跑、不宣称 |
| 建议归属 | World Adapter M2-5；与 Desktop 联调另卡 |

---

## 6. 非目标（本卡遵守）

- 未改 `crates/`、`web` 业务、契约正文、`features.json` 断言
- 未更新 G15 / nightly 状态
- 未宣称真机语音或生产 MES
