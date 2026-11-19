# nodes：其他电脑上的 agent（节点）

通过 `an-node` 把另一台电脑上的 agent 接入，以 `agent名@节点名` 使用，并可组成执行组。

## Sub-features

- 局域网自动发现（`[nodes].discovery`），或“按地址添加”
- 6 位配对码授权；解除配对（两步确认）
- 节点在线检测（`[nodes].refresh_secs`，默认 15 秒）
- 节点上的 agent 注册为 `agent@节点`
- 执行组：`[agents.assign]` 写多个同类型成员，第一个主力，其余备份；主力忙满交备份，都忙排队，离线跳过
- 同一 `task_id` 贯穿主程序与节点日志

## How to get to it (user POV)

在另一台电脑运行 `an-node`，记下地址与配对码 → 主界面右栏“**节点**”页签 →“局域网中发现的节点”输入配对码点“配对”，或在“按地址添加”填 IP 与配对码。

## Driving it with Playwright / HTTP

```bash
# 启动测试节点（本机另一端口）后：
curl -s localhost:8787/api/nodes | jq .
curl -s -XPOST localhost:8787/api/nodes/pair -H 'content-type: application/json' \
     -d '{"address":"192.168.1.23","code":"757563"}'
curl -s -XDELETE localhost:8787/api/nodes/$NODE_ID
```

页面：`getByRole('tab', { name: '节点' })`；输入框 placeholder“地址，如 192.168.1.23”“配对码”；已配对节点有解除按钮（两步确认）。

## Verify（判据）

- 配对后 `/api/nodes` 的 `paired` 含该节点及其 `agents`；左下角出现 `agent@节点` 行。
- 解除后该节点上的 agent 不可用，不能再接任务。
- 执行组：任务卡“执行者”节显示主力/备份 N，负载满时备份接手（RT-ASSIGN-*）。

## Gotchas

- **节点与主程序之间是明文 HTTP**，仅限可信局域网；没有节点 TLS。
- 网络屏蔽组播时发现不到节点，需用“按地址添加”。
- 执行组成员必须是**同一类型** agent；出错直接报错，**不换人重做**。
- 真实 LAN 多节点未被自动化覆盖，现有 `server/tests/e2e.rs` 的 `node_flow` 用测试节点。
- 浏览器没有专门的节点 E2E，页面上的配对/解除属于手动验证。

## Backed by

源码：`crates/node/`、`crates/runtime/src/nodes.rs`、`registry.rs`（高风险）、`crates/agents/src/remote.rs`、`web/src/components/NodesPanel.tsx`
用例：`crates/server/tests/e2e.rs`（`node_flow`、ACP）；RT-ASSIGN-01～05
验证状态：HTTP/后端覆盖；真实局域网未覆盖。
