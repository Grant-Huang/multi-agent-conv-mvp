# 业务内省 tab HTTP 404 — Red→Green

## 问题
打开「内省」页签显示 `Error: HTTP 404`。

## 根因
新前端会请求 `GET /api/introspection/findings`。若运行中的 `agentnexus` 仍是 **S6 之前的二进制**（未重新编译），该路由不存在；在配置了 `static_dir` 时 ServeDir fallback 返回 **404 + index.html**，前端 `request()` 解析不出 `error` 字段，于是显示裸 `Error: HTTP 404`。

复现：`522ddf2`（S6 合并前）二进制 + 当前 `web/dist` → 面板原文即为 `Error: HTTP 404`。

当前 master 已含 `/api/introspection/*`；用当前代码编译的后端打开内省页为「暂无结论」。

## Red
```text
cd web && npm test -- src/components/IntrospectionPanel.test.tsx
AssertionError: expected 'Error: HTTP 404' not to match /^Error: HTTP 404$/
```

## 修复
- `formatIntroLoadError`：404 时给出「重新编译并重启后端」提示
- 未读轮询在接口失败时停止（对齐运营频道）
- 生产入口基线：`introspection_findings_ok_with_static_fallback`

## Green
```text
cd web && npm test -- src/components/IntrospectionPanel.test.tsx  → 1 passed
CARGO_TARGET_DIR=target cargo test -p an-server --test host_guard introspection_findings_ok_with_static_fallback --locked → ok
npx playwright test e2e/intro.spec.ts → 4 passed
旧二进制 + 新前端：面板显示「内省接口不可用（HTTP 404）。…重新编译并重启后端…」
新二进制：面板显示「暂无结论」
```
