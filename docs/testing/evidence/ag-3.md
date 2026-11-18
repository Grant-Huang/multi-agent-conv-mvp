# AG.3 助手页签与卡片证据

日期：2026-11-17。用户批准：AG.3 方案；卡片这次不写进会话历史（放到 AG.4）；"首次"按浏览器记录；页签名为「助手」；使用水平暂时只实现"专业"。

## Red → Green

浏览器用例 `web/e2e/agents.spec.ts`（Playwright，受控模型 + 真实后端 / SQLite / 模拟 ACP agent）：

- 实现前：7 条全部失败。E2E-AGFIRST 失败在"发现的助手"卡片不存在（`element(s) not found`）；其余 6 条逐条单独运行，均失败在找不到「助手」页签（`locator.click` 超时）。服务正常启动，属于功能不存在，不是环境失败。
- 实现后（用例未改）：`7 passed`。
- 全部浏览器用例：`21 passed`（7 条新增 + 原有 14 条），确认首次发现卡没有影响原有界面用例。

测试框架改动（`web/e2e/harness.mjs`，测试替身，不是断言）：用包装脚本把 `claude-agent-acp` 指向自带的模拟 ACP agent（握手真实）；config.toml 手写 agent `hand`；含"改代码验证"的请求路由到 coding；"没有可用的执行者"时的受控回复。

## 改动

- 前端：`AgentsPanel.tsx`（「助手」页签：状态、来源、用途，接入 / 停用 / 恢复启用 / 设为默认 / 移除 / 撤销上一次修改 / 添加自定义助手）、`AgentCards.tsx`（发现卡、引导卡、确认卡、结果卡；检测结果翻译成大白话；复制按钮失败时提示手动选择）、`App.tsx`（页签与首次发现卡）、`api.ts`、`styles.css`。卡片只按结构化数据渲染，不渲染模型生成的 HTML。
- 后端（只读）：`CatalogItem` 增加 `source`、`disabled`（已重新生成 TS 类型）；新增 `GET /api/agents/config`（托管内容、能否撤销、config.toml 里的 agent 与角色、当前执行组）。
- 使用水平：浏览器本地 `an.uiLevel`，默认"专业"；卡片接收这个属性，"普通"模式未实现。

未覆盖：卡片写进会话历史（AG.4）；窄屏下「助手」页签的专门适配（沿用右侧栏现有行为）；真实 claude / codex / cursor / zcode 的接入。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked --no-fail-fast：449 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：exit=0
web npm test：Tests 21 passed (21)
web npm run build：✓ built in 248ms
web npm run test:map：Feature map valid: 22 features, 21 browser cases
web playwright（全部浏览器用例）：21 passed
```

偶发失败记录：第一次 `cargo test --locked` 中原有用例 `host_guard` RT-HOST-02 报 `ConnectionReset`（服务端拒绝请求后提前关闭连接、客户端读取时收到重置）；单独重跑 6 次均通过，随后完整运行 449 个全部通过。本次没有改动 Host 防护相关代码，该用例未修改；它对连接关闭时序敏感，建议另行评估。

本地结果不代替 GitHub CI。
