# World Adapter M0-4 证据

分支：`cursor/wa-m0-4-mom-workbench-f2df`（叠在 M0-3）。

## 范围

- 桌面入口「MOM 工作台」：看板 / 任务 / 资源三入口 + 统一事项页。
- 种子世界与事项共用 `world_id` / `case_id` 约定；视图级标注 模拟 / DES。
- 正式用例：`E2E-WA-01` → `E2E-WABIND`（`web/e2e/mom-workbench.spec.ts`）。

## 结果

| Case | 状态 |
|---|---|
| E2E-WABIND | Green |

## 命令

```text
cd web && npm run build
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome \
  npx playwright test e2e/mom-workbench.spec.ts
# → 1 passed (E2E-WABIND: 三入口打开同一事项且世界隔离)
```

## 非目标

不接真实 DES MCP；不改 `engine.rs`；WA-02～10 其余切片随后续里程碑；事项页完整 DAG/审批仍经运营频道。
