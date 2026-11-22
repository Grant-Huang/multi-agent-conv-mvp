# 证据：T-70 profileStore → B3 preferences

- **日期**：2026-11-22（America/Vancouver）
- **授权**：T-70；`profileStore` → `GET/PATCH /api/desktop/v1/preferences`
- **Gap / Mock**：SMT-03；MOCK_REGISTRY **M3** 助理配置 UI 接真
- **范围**：仅前端 `api.ts` / `profileStore` / `SettingsPanel` / `App.tsx` + 文档；**未改** `store.rs` / `orchestrator.rs` / `desktop_api` 业务路径
- **模式**：真实 B3 HTTP；记忆列表仍为本地种子；冲突注入=服务端升版夹具（`expected_version` 空 PATCH）

## Case

| Case | 场景 |
|---|---|
| E2E-ADPROF | 保存同步名称；记忆返回；冲突不覆盖草稿 |
| RT-ADPREF-01～03 | 回归 |

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_b3 --config /tmp/cargo-noproxy.toml
→ 7 passed（含 RT-ADPREF-01～03）

cd web && npm run build
→ 通过

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADPROF'
→ 1 passed（E2E-ADPROF；真实 B3 HTTP）
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 保存后侧栏/首页名称同步 | E2E-ADPROF | 通过 |
| 记忆面包屑返回助理分类 | E2E-ADPROF | 通过 |
| 冲突提示且草稿未被存档覆盖 | E2E-ADPROF | 通过 |
| GET 默认 / PATCH 升版 / 409 | RT-ADPREF-01～03 | 通过 |

## 非本卡

- 记忆建议列表 / C2 投递 UI
- 已读跨端
- 真机 mic
