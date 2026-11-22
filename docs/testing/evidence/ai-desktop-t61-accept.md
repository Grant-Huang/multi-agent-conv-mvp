# 证据：T-61 AcceptMockPanel → B1 真实受理

- **日期**：2026-11-22（America/Vancouver）
- **授权**：T-61；选① Accept → `/api/desktop/v1/.../requests`
- **Gap / Mock**：SMT-03；MOCK_REGISTRY **M1** 受理 UI 接真
- **范围**：仅前端面板 + `api.ts`；**未改** store / orchestrator / 公开字段
- **模式**：真实 B1 HTTP；「丢回执」为客户端夹具（服务端已 202，UI 标 unknown 再 GET 回读）

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_accept
→ 4 passed（RT-ADACCEPT-01～04）

cd web && npm run build
→ tsc + vite build 通过

cd web && npm run test:map
→ Feature map valid

PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright \
  npx playwright test e2e/desktop.spec.ts -g 'ADACCEPT'
→ 1 passed（E2E-ADACCEPT；真实 HTTP）
```

## 断言对照

| 场景 | Case | 结果 |
|---|---|---|
| 成功受理 / 幂等重试 | E2E-ADACCEPT 可靠段；RT-ADACCEPT-01/02 | 通过 |
| 服务拒绝（同 ID 改正文） | E2E 409；RT-ADACCEPT-03 | 通过 |
| 丢回执恢复（回读 / 可靠重试） | E2E legacy 回读 + reliable 重试；RT-ADACCEPT-04 | 通过 |

## 非本卡

- Resume / profile 接真（②③）
- 删 `acceptMock.ts` 源文件（面板已不引用）
