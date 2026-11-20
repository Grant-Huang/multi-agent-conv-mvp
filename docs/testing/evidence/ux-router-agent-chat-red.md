# 体验 PR3 对话路由与助手话术：Red 证据

日期：2026-11-20。

冻结用例（实现前）：

- `an-runtime` 单元：`rt_agchat_12_mgmt_hint_catches_manage_and_config`、`rt_agchat_14_howto_specify_executor` 失败（`mgmt_hint` 占位返回 `None`）。
- `an-runtime` 集成：`rt_agmgmt_01_rejects_agent_management_goals` 在 Router 未拦截时落到不可达 LLM（业务断言失败）。
- 守护：`mgmt_hint_ignores_use_agent_work_and_file_reads` 在占位实现下已通过（全为 `None`）。

业务断言失败，非环境问题。实现确定性拦截与话术后应转绿（见 `ux-router-agent-chat.md`）。
