# 06 排查与反馈

## 场景 A：先看连接状态

左下角：LLM 与每个 agent 一行（绿/灰/红）。点“连接详情 ▾”可看后端地址、实际模型名、结构化输出方式。后端未连接时显示“后端未连接”。每 10 秒自动刷新。

## 场景 B：打开开发者模式，看诊断与路由日志

右侧底部点“**开发者模式：关**”切为“开”（偏好保存在浏览器本地，默认关闭）。多出两个页签：

**诊断**（点“刷新”更新）

| 区块 | 看什么 |
|---|---|
| 后端 | 模型服务连通性、槽位占用 |
| 执行组 | 每个角色的成员、状态；`agent id@节点` 的准确写法 |
| 问题统计（最近 500 条） | 错误码计数 |
| 最近的问题 | 事件、错误码、关联 task |

“复制诊断信息”可直接贴给 AI 或开发者。

**路由日志**：每一轮“前台判断 + 路由”的决策，可展开看模型原始输出。判断为什么派给了这个角色、风险为何这么估，看这里。

## 场景 C：查 JSON 日志

日志在 `logs/agentnexus.jsonl.<日期>`（节点在 `~/.agentnexus-node/logs/`）。每条有固定 `event`（事件名）、`code`（错误码）并关联 `task_id`。

```bash
jq -c 'select(.task_id == "task-xxxx") | [.timestamp, .event, .status, .code]' logs/agentnexus.jsonl.*
```

- 配置里字段名写错时，启动日志出现 `W_CONFIG_UNKNOWN_KEY`。
- 访问被拒（Host 不合法）会记 WARN `http.host_rejected` / `E_HTTP_HOST`，只记 Host 前 100 字符与路径，不记请求体和其他头。
- 日志目录不可写时降级为控制台/内存诊断，不影响服务。
- 完整事件与错误码见 [DESIGN.md 第 10 节](../DESIGN.md#10-日志与排查)。

## 场景 D：访问页面被拒绝（403）

主服务只接受 Host 为 `localhost`、`127.0.0.1`、`[::1]` 且**端口与实际监听端口一致**的请求（防 DNS 重绑定），不信任转发头。用反向代理或改了 hosts 访问会被拒。**这不是登录鉴权。**

## 场景 E：觉得系统“派错了人”，反馈一下

1. 展开可匹配到用户消息的任务卡片；
2. 点“**派错了**”；
3. 选正确的动作/角色，写“一句说明”，点“保存”；
4. 提示“已记下”。

反馈保存在数据库（`POST /api/feedback`），非法动作/角色会被拒；导出只是草稿，**不会自动改评测集**。积累后可用于 [07](07-model-and-eval.md) 的路由评测。

## 场景 F：常见故障速查

| 现象 | 原因 | 处理 |
|---|---|---|
| LLM 红色 | 模型服务未启动/地址错 | 查 `[llm].base_url`；`curl <base_url>/models` |
| llama.cpp 返回 400 | 不支持该 `response_format` 写法 | 换 `structured_mode`（`json_schema` → `json_object` → `prompt_only`） |
| Coding agent 红色 | 命令不存在 | 查 `[agents.coding].command` 或接入 ACP 助手，见 [03](03-connect-agents.md) |
| agent 灰色 | 已配置但握手失败 | 助手页签点该行“排查” |
| 响应很慢 | 推理类模型先思考 | 给 `conversation`/`router` 配更小更快的模型，见 [07](07-model-and-eval.md) |
| 工具循环不收敛 | 小模型 | 调小 `agents.general.max_steps` |
| 任务重启后变失败 | 不支持断点续跑，中断的任务标为失败 | 重新发起 |
