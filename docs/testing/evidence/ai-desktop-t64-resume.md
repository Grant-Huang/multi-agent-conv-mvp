# 证据：T-64 ResumeMockPanel → B2 SSE

- **日期**：2026-11-22（America/Vancouver）
- **授权**：T-64；Resume → `/api/desktop/v1/events`
- **Gap / Mock**：SMT-03；MOCK_REGISTRY **M2**
- **范围**：`desktop_api` fixture（`bus.publish`+`flush`，无新表）+ 前端面板；**未改** `store.rs` / `orchestrator.rs`
- **模式**：真实 B2 SSE；种子/重放/续跑为 harness fixture HTTP

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_replay
→ 5 passed（RT-ADREPLAY-01～05）

CARGO_TARGET_DIR=target cargo build -p an-server --bin agentnexus
→ Finished

cd web && npm run build
→ 通过

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADRESUME'
→ 1 passed（E2E-ADRESUME；真实 SSE）
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 双任务种子不串写 | E2E-ADRESUME | 通过 |
| A 断线重放进度/事件数 | E2E-ADRESUME | 通过 |
| B waiting→running | E2E-ADRESUME | 通过 |
| SSE 游标契约 | RT-ADREPLAY-01～05 | 通过 |

## 非本卡

- 独立 Run 表 / orchestrator 续跑
- preferences / 真机 mic
