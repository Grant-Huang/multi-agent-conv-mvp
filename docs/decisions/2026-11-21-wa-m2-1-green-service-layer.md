# World Adapter M2-1 Green：服务层接线（助手建议）

- **日期 / 状态**：2026-11-21 / 助手建议（随 M2-1 Red PR；待用户批准后再开 Green）
- **问题**：远程 DES 写路径（Propose/Execute/Confirm）尚未挂在 `/api/world/des`；Green 可能触及高风险模块。
- **定了什么（建议）**：
  - Green 默认只在 `mom_world_des`（及必要的 `main` merge / 测试控件）接线；调用既有 MOM 审批/执行与 `DesConnector::dispatch_writes` 公共 API。
  - **不改** `engine.rs` / `orchestrator.rs` / `registry.rs` / `DesConnector` 内部，除非升级方案再批。
  - M2-1 本卡只到 Red（CASES 表格 + Playwright 空壳 + 本地 Rust Red 证据）；Rust 失败测试不入库。
- **为什么**：与 M1-1 服务层优先一致；缩小高风险面，保留 Mock 写路径作回归。
- **没选什么**：本卡直接改引擎提交窗口或 DesConnector 超时/锁；本卡写 Green。
- **出问题先看**：[`WA_M2_1_PLAN.md`](../mom/WA_M2_1_PLAN.md)、[`wa-m2-1-red.md`](../testing/evidence/wa-m2-1-red.md)。
