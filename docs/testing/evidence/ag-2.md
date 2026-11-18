# AG.2 托管配置、热注册与撤销证据

日期：2026-11-17。用户批准：AG.2 方案（含修改高风险文件 `registry.rs`）；撤销保留 10 个版本；环境变量只允许写引用；停用某角色最后一个 agent 时允许，不改派，由主 agent 用非专业用户能看懂的话回复；修复原有用例时序依赖的方案 A。

## Red → Green

`CARGO_TARGET_DIR=target cargo test --locked -p an-server --test agent_config`

- 实现前（只有让测试能编译的空骨架）：`0 passed; 14 failed`。失败均为业务断言：提议接口 404；RT-AGCFG-09 未报告托管文件错误；RT-AGCFG-13 回复仍为模板"这个任务暂时无法派发：[E_ROUTE_NO_AGENT] …"；RT-AGCFG-14 交付提示词里没有大白话要求。
- 实现后：`ok. 14 passed; 0 failed`。
- 另加单元测试：`agent_config::tests` 3 条（环境变量引用格式、执行组合并以 config.toml 为准、托管文件 TOML 往返）。

## 与原有用例的两处协调（如实记录）

1. **e2e `node_flow`**（原有，未改）要求"没有可用 agent 时回复里如实带 `E_ROUTE_NO_AGENT`"。改为由前台模型生成回复后，模型的话里不一定有错误码，这条用例失败。处理：改业务代码——模型回复没提到错误码时，在**末尾括号**里补"（技术信息：E_ROUTE_NO_AGENT）"。同时把本次新写的 RT-AGCFG-13 的断言由"回复完全等于模型输出"放宽为"以模型输出开头、错误码只在末尾括号里"（这条用例是本次新写，原断言过严）。
2. **`supplement_routing` RT-CONV-01**（原有）在 policy 模式失败：断言"补充路由失败后旧任务事件不变"，但快照在事件异步落库（最多 20ms）之前拍下。失败信息中"多出"的两条事件时间戳都是第一条消息的时刻，不是补充要求产生的。经用户批准（方案 A），在两次读取前各调用已有的 `rt.store.flush_events()`；断言本身未改，第二次读取前也先落库，比较因此更严格。修改后该文件 `2 passed`。

## 改动与边界

- 托管文件 `agents.managed.toml`（数据库同目录；数据库在内存里时不支持修改）。合并顺序：config.toml → 托管 → 自动发现的预设，同 id 先到先得；config.toml 手写的 agent 和执行组不能在对话里改（409，提示改 config.toml）。
- 写入：确认串行（tokio 锁）；临时文件 + 刷盘 + 改名；修改前存历史，最多 10 个；撤销是提议类型 `revert`，可连续撤销；没有版本时 409。
- 文件损坏：启动不失败，`load_error` 带 `E_AGENT_MANAGED_INVALID`，新提议 409，坏文件不被覆盖。
- 凭证：提议返回一次性凭证，10 分钟有效；错误 403、过期 410、缺失 422。只防系统内模型自我批准，挡不住本机其他能访问 HTTP 接口的程序（与"批准任务"接口相同）。
- `registry.rs`：新增 `unregister_local`（只移除本机条目，执行中的名额照常使用和归还，排队的任务被唤醒重新挑选）、`set_assign`、`assign`、纯函数 `check_group`、`role_label`；"没有 agent"的原因改为大白话。
- 后台探测列表改为 `ProberSet`（可增删，探测循环每轮取快照）；新接入的 agent 在确认时立即握手检测一次（沿用 10 秒超时），结果写入连接状态并返回给界面。
- 锁顺序：确认锁 → 状态锁 → 注册表锁；注册表锁内不做 I/O。
- 实现调整：撤销不设单独接口，改为提议类型 `revert`；停用确认卡的警告没有给出排队任务的具体数量（留到 AG.3）。

未验证：真实 claude / codex / cursor / zcode agent 的热接入与握手（沙箱未安装）；界面卡片（AG.3）。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked：449 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：exit=0（此前报 1 处 items_after_test_module，已修）
web npm test：Tests 21 passed (21)
web npm run build：✓ built in 397ms
web npm run test:map：Feature map valid: 21 features, 14 browser cases
```

本地结果不代替 GitHub CI。
