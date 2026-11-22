# T-63 M2 合入后跨组独立复验

| 项目 | 内容 |
|---|---|
| 任务 | T-63（独立验收人；不改业务代码） |
| 验收人 | 本 worker（未实现 #205/#206/#207） |
| 基线 | `origin/master` `afb69d6`（含 #204 T-60、#205 契约、#206 M2-5、#207 M2-6、#203 T-61 Accept） |
| 日期 | 2026-11-22（UTC） |
| 口径 | 合同 / Playwright harness = fixture·Mock·服务层；远程真 `des_mcp` = **NOT_VERIFIED**（本卡非目标） |
| 目标 | 跨组写路径可独立复验通过 |

## 结论

| 链路 | 等级 | 说明 |
|---|---|---|
| Desktop C2：RT-ADAPPR/ADATTN + E2E | **VERIFIED / T2** | 7 + 2 passed；先 `npm run build` |
| Desktop Accept：RT-ADACCEPT + E2E-ADACCEPT | **VERIFIED / T2** | 4 + 1 passed（#203） |
| WA 写合同 RT-WADES-09～19 + m25 | **VERIFIED / T2** | `mom_world_des_write` 12 passed |
| E2E-WAEXEC/WARECV/WADYN/WALOOP | **VERIFIED / T2** | 4 passed（#206 后独立复跑） |
| `wa-m2-regression` | **VERIFIED / T2** | write12 + read12 + DES4 + DESC6 |
| 契约对照（#205） | **对齐** | 见 §4；未改契约正文 |
| 远程真 DES | **NOT_VERIFIED** | `AN_MOM_DES_MCP` unset；非本卡 |
| master push @ `afb69d6` | **部分** | web/python success；`security` **failure**（`cargo audit`：`url` RUSTSEC-2026-0592）；rust 复验时仍 in_progress — 见 Q-T63-01 |

**总体（本卡目标）**：跨组写路径独立复验 **PASS**。远程真厂与依赖审计漏洞另卡。

---

## 1. 本地证据

前置：`cd web && npm run build`（关闭 Q-T60-01 复现路径）。

### 1.1 Desktop

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_c2 --test desktop_accept --locked
desktop_accept: 4 passed（RT-ADACCEPT-01～04）
desktop_c2:     7 passed（RT-ADAPPR-01～05、RT-ADATTN-01～02）

$ cd web && npx playwright test e2e/desktop.spec.ts -g 'ADAPPR|ADATTN|ADACCEPT'
✓ E2E-ADATTN / E2E-ADAPPR / E2E-ADACCEPT
3 passed
```

### 1.2 WA 写路径

```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test mom_world_des_write --locked
12 passed（含 RT-WADES-09～19、m25_official_by_world_id_delivers_ops）

$ cd web && npx playwright test e2e/wa-des-write.spec.ts
✓ E2E-WAEXEC / WARECV / WADYN / WALOOP
4 passed

$ ./scripts/wa-m2-regression.sh
write 12 + read 12 + RT-DES 4 + RT-DESC 6 → wa-m2 regression green
```

Mock 标记：harness `world_mock` / 进程内 DES 服务层；不宣称远程 MCP。

---

## 2. 合入 PR / master CI（摘录）

| 项 | 状态 |
|---|---|
| #204～#207 已合入 `afb69d6` | 是 |
| #206/#207 作者证据 | `wa-m2-5-browser.md` / `wa-m2-6.md` |
| push CI `afb69d6` | web/python-* success；security **fail**（url 2.5.8 → 需 ≥2.5.9）；非本卡业务复验失败 |

---

## 3. 契约对照（#205 / `docs/contracts/`）

| 不变量主题 | 复验 Case | 结果 |
|---|---|---|
| 批准绑定版本（A1/A2） | RT-ADAPPR-01/02；RT-WADES-09 | 通过 |
| 一次消费 / 幂等（A3） | RT-ADAPPR-04；RT-WADES-10 | 通过 |
| 取消待发不发（S3） | RT-WADES-11；E2E-WAEXEC | 通过 |
| unknown / 关联键（S6） | RT-ADAPPR-05；RT-WADES-12/15；E2E-WARECV | 通过 |
| 干预前缀/未来（WA-05） | RT-WADES-14；E2E-WADYN | 通过 |
| 双角色同一 execution_id（WA-08） | E2E-WALOOP | 通过 |
| Accept 可靠受理 | RT-ADACCEPT-*；E2E-ADACCEPT | 通过 |

未改契约正文；语义与「部分有效（C2/M2 写路径）」一致。

---

## 4. Q-T60-01～03 处置

| ID | 原问题 | 本轮 | 关闭理由 / 证据 |
|---|---|---|---|
| Q-T60-01 | 缺 `web/dist` 白屏 | **关闭** | 本会话先 `npm run build` 再跑 Playwright；Desktop 3 + WA 4 全绿 |
| Q-T60-02 | 远程真 DES | **遗留 NOT_VERIFIED** | 仍无 `AN_MOM_DES_MCP`；T-63 非目标 |
| Q-T60-03 | WA 写路径浏览器 E2E 属 M2-5 | **关闭** | #206 已合；本轮独立复跑 E2E-WA* 4 passed |

### Q-T63-01（新）— master push `security` 失败

| 字段 | 内容 |
|---|---|
| 提交 | `afb69d6` |
| 现象 | `cargo audit`：`url` 2.5.8 / RUSTSEC-2026-0592 |
| 与本卡 | 不阻塞写路径业务复验结论 |
| 建议归属 | 工程/依赖治理另卡升级 `url` ≥2.5.9 |

---

## 5. 相对目标的差距（更新）

目标 = **跨组写路径可独立复验通过**。

| 已达成 | 仍差距 |
|---|---|
| Desktop C2 + Accept 合同与桌面 E2E | 远程真 `des_mcp` live |
| WA M2 合同 + 浏览器写闭环 E2E | M0/M1 全量套件本轮未再跑 `wa-m2-suite` 整包（regression + 四条 E2E 已覆盖 M2 写路径；全量套件可另补） |
| 契约最小集已合入且复验对齐 | master `security` audit 红（依赖） |
| Q-T60-01/03 关闭 | Q-T60-02；M3 生产只读非目标 |

---

## 6. 非目标（遵守）

未改 crates/web 业务、契约正文、`features.json` 断言；未改 engine/DesConnector；未跑远程真 DES；未宣称 M3。
