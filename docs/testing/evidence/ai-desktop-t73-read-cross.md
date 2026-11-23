# 证据：T-73 已读跨端接服务（M3）

- **日期**：2026-11-22（America/Vancouver）
- **授权**：#219 合入后主线；已读状态跨端/服务权威
- **范围**：`desktop_api` 进程内夹具（GET messages / GET|POST read / reset，**无新表**）+ `AssistantReadPanel`；**未改** `store.rs` / `orchestrator.rs`
- **模式**：夹具可辨；`POST .../assistant/messages/fixture/reset` 恢复种子

## Case

| Case | 场景 |
|---|---|
| E2E-ADXR | 双上下文标已读；未读徽标跨端一致；重复提交幂等 |
| RT-ADXR-01～04 | GET 列表 / POST 标已读 / 幂等 / 空列表 400 |

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_read --config /tmp/cargo-noproxy.toml
→ 4 passed（RT-ADXR-01～04）

cd web && npm run build
→ 通过

cd web && npm run test:map
→ Feature map valid

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADXR'
→ 1 passed（E2E-ADXR）

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts
→ 19 passed
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 列表未读种子 | RT-ADXR-01 / E2E-ADXR | 通过 |
| POST 标已读 | RT-ADXR-02 / E2E-ADXR | 通过 |
| 重复提交幂等 | RT-ADXR-03 / E2E-ADXR | 通过 |
| 空 message_ids 400 | RT-ADXR-04 | 通过 |
| 双上下文跨端一致 | E2E-ADXR | 通过 |

## 非本卡

- 真机 mic；持久化记忆表/抽取；阅读锚点 localStorage→服务；整壳 UI/UE
