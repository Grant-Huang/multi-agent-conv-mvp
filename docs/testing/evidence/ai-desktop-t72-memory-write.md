# 证据：T-72 记忆纠错/撤回写路径（M3）

- **日期**：2026-11-22（America/Vancouver）
- **授权**：#217 合入后主线；记忆纠错/撤回写路径
- **范围**：`desktop_api` 进程内夹具（PATCH/DELETE + reset，**无新表**）+ SettingsPanel；**未改** `store.rs` / `orchestrator.rs`
- **模式**：夹具可辨；`POST .../memories/fixture/reset` 恢复种子

## Case

| Case | 场景 |
|---|---|
| E2E-ADMEMW | 纠错正文；撤回后列表隐藏；草稿保留 |
| E2E-ADMEM | 回归只读路径 |
| RT-ADMEM-04～06 | PATCH / DELETE 幂等 / 404 |

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_mem --config /tmp/cargo-noproxy.toml
→ 6 passed（RT-ADMEM-01～06）

cd web && npm run build
→ 通过

cd web && npm run test:map
→ Feature map valid

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADMEM'
→ 2 passed（E2E-ADMEM、E2E-ADMEMW）

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 18 passed
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| PATCH 更新正文 | RT-ADMEM-04 / E2E-ADMEMW | 通过 |
| DELETE 列表隐藏且幂等 | RT-ADMEM-05 / E2E-ADMEMW | 通过 |
| PATCH 未知 404 | RT-ADMEM-06 | 通过 |
| 草稿保留 | E2E-ADMEMW | 通过 |

## 非本卡

- 真机 mic；已读跨端；持久化记忆表；完整 Taste 建议流
