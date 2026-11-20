# 运营桌面 D2（卡片批准）：证据

日期：2026-11-20。决定 60。

## Red
见 `ops-desk-d2-red.md`。

## Green
```text
$ CARGO_TARGET_DIR=target cargo test -p an-server --test mom_ops
test rt_opsdesk_10_… ok
test rt_opsdesk_11_approve_is_idempotent_and_rejects_stale ... ok

$ cd web && PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test ops.spec.ts
  6 passed（含 E2E-OPSAPPROVE）
```

实现：`OpsCase` 增字段并重生 bindings；`POST …/approve`（幂等 already、过期 409、固定 supervisor）；`OpsCards` 批准按钮；立刻 `refresh_case`。
