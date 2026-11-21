# AI Desktop A5 整体收口证据（M1–M6 Mock 贯通）

日期：2026-11-20（America/Vancouver）

## 范围

在 A5.1–A5.4 分卡合入后，于最新 `master` 做 A5 出口核对：

- M1 受理 Mock（`E2E-ADACCEPT`）
- M2 Run 恢复 Mock（`E2E-ADRESUME`）
- M3 助理/设置（`E2E-ADPROF`，A4 已接，本回归复跑）
- M5 审批 + Attention（`E2E-ADAPPR`、`E2E-ADATTN`）+ S1（`E2E-ADASK`）
- M6 语音 / 跨端（`E2E-ADVOICE`、`E2E-ADHAND`）
- 同文件既有 A1–A4 桌面用例一并回归（`desktop.spec.ts` 全量）

**非本卡**：B1 真实可靠受理 HTTP、B2 SSE/重启对账、C1 组织 ACL 撤权夹具、D 真实媒体。

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 59 browser cases |
| desktop.spec.ts 全量 | **15 passed**（约 9.6s） |
| build | ok |

### desktop.spec 用例清单（全绿）

`ADNAV`、`ADLAYOUT`、`ADDRAFT`、`ADSCOPE`、`ADEVID`、`ADHISTORY`、`ADPROF`、`ADINPUT`、`ADASK`、`ADATTN`、`ADAPPR`、`ADRESUME`、`ADVOICE`、`ADHAND`、`ADACCEPT`

### 分卡证据

| 卡 | 证据 |
|---|---|
| A5.1 | `docs/testing/evidence/ai-desktop-a5.1.md` |
| A5.2 | `docs/testing/evidence/ai-desktop-a5.2.md` |
| A5.3 | `docs/testing/evidence/ai-desktop-a5.3.md` |
| A5.4 | `docs/testing/evidence/ai-desktop-a5.4.md` |

## 后续（不在本卡实现）

| 项 | 里程碑 |
|---|---|
| 可靠受理 HTTP / 操作 ID 与真实 `/api/conversations*` 对齐 | B1 |
| Run SSE 重连与进程重启对账 | B2 |
| 个人配置/关联持久 API | B3 |
| RelOS/证据权限撤权夹具与组织 ACL | C1 |
| 动作网关真实副作用 | C2 |
| 真实麦克风与双设备续接 | D1/D2 |
