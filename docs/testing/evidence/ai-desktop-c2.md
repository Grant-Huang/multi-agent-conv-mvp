# 证据：AI Desktop C2 — 动作网关 + Attention

- **日期**：2026-11-21（America/Vancouver）
- **决定**：[2026-11-21-ai-desktop-c2-gateway-attn](../../decisions/2026-11-21-ai-desktop-c2-gateway-attn.md)
- **范围**：desktop_approvals + attention；不改 orchestrator；不并 OpsCase；不镜像决定 69

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_c2 --locked
→ 7 passed（RT-ADAPPR-01～05、RT-ADATTN-01～02）

cd web && npx playwright test e2e/desktop.spec.ts
→ 16 passed（含 E2E-ADAPPR、E2E-ADATTN）

cd web && npm run build && npm run test:map
→ build 通过；Feature map valid

CARGO_TARGET_DIR=target cargo clippy --all-targets --locked
→ Finished（desktop_c2 无告警）

CARGO_TARGET_DIR=target cargo test --locked
→ 未运行（全量；本卡以 desktop_c2 + desktop.spec 为硬门禁）
```

## 断言摘要

| Case | 结果 |
|---|---|
| RT-ADAPPR-01 | approve 匹配 revision → approved |
| RT-ADAPPR-02 | bump 后旧 revision execute → 409；count=0 |
| RT-ADAPPR-03 | reject 后 execute 拒绝；count=0 |
| RT-ADAPPR-04 | execute 两次 → count=1 |
| RT-ADAPPR-05 | unknown → ack → applied；count=1 |
| RT-ADATTN-01 | mute 丢 ordinary；urgent 入队 |
| RT-ADATTN-02 | ordinary 同 title 去重为 1 |

## 非本卡

- 真实 NexusOps 厂写；完整记忆 UI；IdP；决定 69 镜像
