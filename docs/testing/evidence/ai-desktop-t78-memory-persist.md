# 证据：T-78 记忆持久化表

- **日期**：2026-11-23（America/Vancouver）
- **授权**：#236 合入视为批准方案；决策 `docs/decisions/2026-11-23-ai-desktop-t78-memory-persist.md`
- **范围**：`desktop_memories` + Store；API 接 Store；`fixture/reset` 写库复位；**未改** orchestrator；未扩 DDL 超方案
- **契约**：RT-ADMEM-01～06 / E2E-ADMEM/ADMEMW 断言不变；新增 RT-ADMEM-07 重启持久化

## Case

| Case | 结果 |
|---|---|
| RT-ADMEM-01～06 | 保留绿 |
| RT-ADMEM-07 | PATCH 后同库新 Runtime GET 仍见纠错正文 |
| E2E-ADMEM / E2E-ADMEMW | 回归 |

## 命令与结果

```text
# America/Vancouver 2026-11-23
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_mem --locked
→ 7 passed（含 RT-ADMEM-07）

CARGO_TARGET_DIR=target cargo clippy -p an-runtime -p an-server --all-targets --locked -- -D warnings
→ Finished（无警告阻断）

cd web && npm run build && npm run test:map
→ build OK；Feature map valid: 33 features, 89 browser cases

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADMEM'
→ 2 passed（E2E-ADMEM、E2E-ADMEMW）
```


## 非本卡

阅读锚点服务化；RelOS/抽取；IdP；ASR；双物理机
