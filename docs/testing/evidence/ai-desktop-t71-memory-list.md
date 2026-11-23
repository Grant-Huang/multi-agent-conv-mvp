# 证据：T-71 记忆列表接服务（M3）

- **日期**：2026-11-22（America/Vancouver）
- **授权**：主线下一刀；MOCK_REGISTRY **M3** 记忆列表仍本地种子 → 服务权威
- **范围**：`desktop_api` 夹具 `GET /memories`（无新表）+ 前端 SettingsPanel；**未改** `store.rs` / `orchestrator.rs`
- **模式**：服务端夹具种子（与 A4 演示条目同内容）；非完整个人记忆抽取/纠错；非已读跨端

## Case

| Case | 场景 |
|---|---|
| E2E-ADMEM | 列表→详情→返回；正文来自 HTTP |
| E2E-ADPROF | 回归（记忆返回路径） |
| RT-ADMEM-01～03 | 列表 / 详情 / 404 |

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_mem --config /tmp/cargo-noproxy.toml
→ 3 passed（RT-ADMEM-01～03）

cd web && npm run build
→ 通过

cd web && npm run test:map
→ Feature map valid

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADPROF|ADMEM'
→ 2 passed

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 17 passed（含 E2E-ADMEM）
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 列表含 mem-1/mem-2 标题 | E2E-ADMEM / RT-ADMEM-01 | 通过 |
| 详情正文/来源/有效期 | E2E-ADMEM / RT-ADMEM-02 | 通过 |
| 未知 id 404 | RT-ADMEM-03 | 通过 |
| ADPROF 记忆返回路径 | E2E-ADPROF | 通过 |

## 非本卡

- 真机 mic
- 记忆建议接受/拒绝 UI（C2 Attention 已另接）
- 记忆纠错/撤回/设有效期写路径
- 已读跨端
- 持久化记忆表 / store schema
