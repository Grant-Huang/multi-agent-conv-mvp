# AI Desktop A4 证据（助理设置 / 交互与响应式）

日期：2026-11-20（America/Vancouver）

## 范围

- 设置面板：分类 → 助理身份 → 记忆详情；Esc/返回上层
- 助理配置 Mock（M3）：localStorage 版本冲突不覆盖编辑草稿
- 工作区「调整助理」、侧栏 chip、首页助理名同步
- 响应式入口（360/768/1024/1440）、IME composition 不发送、`prefers-reduced-motion` 根节点标注

## 命令

```bash
cd web && npm run test:map && npm run build
PLAYWRIGHT_BROWSERS_PATH=$HOME/Library/Caches/ms-playwright npx playwright test desktop.spec.ts
```

## 结果

| 项 | 结果 |
|---|---|
| test:map | 32 features, 49 browser cases |
| desktop.spec（含 ADPROF/ADINPUT） | 8 passed |
