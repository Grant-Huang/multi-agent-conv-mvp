# World Adapter M4-0 — 方案与准入清单（无业务代码）

分支：`cursor/wa-m4-0-write-gates-f2df`。任务卡：[WA_M4_TASKS.md](../../mom/WA_M4_TASKS.md)。决定：[2026-11-23-wa-m4-write-entry-gates.md](../../decisions/2026-11-23-wa-m4-write-entry-gates.md)。

## 范围

- 冻结 M4 切片表（M4-0～6）与 **准入门槛 G1～G8**
- 写明与 M3 桩路径边界；保持 `WRITE_DISABLED`
- 将「候选站只读全浏览器路径」标为 **M4-1（待授权）**，作为写实现前置
- 同步 PLAN / 注册表 / Feature Map / features.json sources

## 结论摘要

| 项 | 状态 |
|---|---|
| M3 桩数据读准出 | 已完成（#228 / `wa-m3-5.md`） |
| 候选站 / 权威源 / 身份映射 | **未就绪**（G1～G3） |
| 候选站只读浏览器路径 | **未做** → 下一待授权卡 M4-1 |
| MES 真写实现 | **本卡不做** |
| `WRITE_DISABLED` | **保持**（G8） |

## 地图校验

```text
$ cd web && npm run test:map
Feature map valid: 33 features, 80 browser cases
```

（本卡不新增 CASES 表格行 / 不新增 Playwright；仅 sources 挂任务卡与证据。）

## 非目标

不改 `engine` / `store` / `orchestrator`；不接真站写；不绕过 `WRITE_DISABLED`；不自开 M4-1。
