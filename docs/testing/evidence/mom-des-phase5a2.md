# 模拟工厂连接器 阶段⑤a-2 Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（master 8bede3a 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §11a"⑤a-2 设计"；决策：[DECISIONS.md](../../DECISIONS.md) DES-27；用例：[CASES.md](../CASES.md)「模拟工厂连接器（阶段⑤a-2）」。

范围：本提交只包含以下内容，行为尚未实现：
- 用例；
- 设计与决策；
- 骨架：
  - 引擎远程模式 `crates/runtime/src/mom/des/remote.rs`，方法都直接报"尚未实现"；
  - 服务层连接器 `crates/server/src/mom_des_connector.rs`，启动即报 `E_DES_UNAVAILABLE`；
- 失败测试。

没有修改已有测试断言。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_connector
test rt_desc_01_mcp_mode_matches_the_in_process_engine ... FAILED
test rt_desc_02_writes_only_after_approval_and_idempotent ... FAILED
test rt_desc_03_agents_only_see_observation_tools ... FAILED
test rt_desc_04_offline_factory_fails_loudly_without_fallback ... FAILED
test rt_desc_05_review_truth_comes_from_the_factory_admin_tool ... FAILED
  the simulated factory starts: E_DES_UNAVAILABLE: connector is not implemented yet
test result: FAILED. 0 passed; 5 failed
```

失败原因是连接器尚未实现（启动即返回错误），不是环境问题：`an-des-mcp` 已构建，可执行文件存在。

## 其他检查（同一提交）

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 554 passed，5 failed（只有上述 5 个新用例），exit=101
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 27 features, 24 browser cases
cd web && npm run build                                                   → ✓ built
```

## Green

用户确认用例与 DES-27 后实现，没有修改测试断言。an-mom 接入按用户决定放到下一步。

- 引擎远程模式（`crates/runtime/src/mom/des/remote.rs`）：
  - 远程仿真只登记工厂结构与场景，不保存重放输入；
  - `des_ingest_remote` 把事件入队，并保存当时的快照；
  - 执行后，按与进程内相同的规则算出干预，排成待发写操作，并记录 `DesPlanApplied`；
  - 回执写回动作（完成时刻、失败与补偿、单号）；
  - 标准答案由连接器写入。
- `engine_link.rs`：
  - 观测刷新时，远程仿真用连接器交来的快照，且只在快照的观测时刻与当前处理的分钟一致时写入；
  - 执行回调遇到远程仿真时，走上述远程分支；
  - 案例关联与协同事项按两种模式统一取工厂结构（`des_factory_config`）。
- 服务层连接器（`crates/server/src/mom_des_connector.rs`）：
  - 经 stdio 启动 `an-des-mcp`，用 rmcp 客户端通信；
  - 逐分钟推进；
  - 发出写操作，并按回执刷新标准答案；
  - 只对 agent 开放观测类工具（来自 `an_des::tool_catalog`，且须是模拟工厂实际提供的）；
  - 失败时统一报 `E_DES_UNAVAILABLE`。

**实施中的一处修正：** 第一次运行 RT-DESC-01 时，P0 的排产完成时刻比进程内晚 1 分钟（798 vs 797）。原因是引擎每次 `advance` 会重新处理当前这一分钟，远程模式在这时提前用了下一分钟的快照。改为只在快照的观测时刻与正在处理的分钟一致时写入后，两种模式完全一致。

```text
CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast   → 559 passed，0 failed，exit=0
  rt_desc_01_mcp_mode_matches_the_in_process_engine ... ok
  rt_desc_02_writes_only_after_approval_and_idempotent ... ok
  rt_desc_03_agents_only_see_observation_tools ... ok
  rt_desc_04_offline_factory_fails_loudly_without_fallback ... ok
  rt_desc_05_review_truth_comes_from_the_factory_admin_tool ... ok
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets              → 无警告
cargo fmt --all -- --check                                                → 通过
cd web && npm run test:map                                                → Feature map valid: 27 features, 24 browser cases
cd web && npm run build                                                   → ✓ built
```

本机磁盘额度有限，以上命令在 `CARGO_PROFILE_DEV_DEBUG=0` 的环境中运行。这个变量只去掉调试信息，不影响测试行为。`Cargo.lock` 只为 `an-server` 新增了 `an-des` 与 `rmcp` 两条依赖记录，没有引入新的第三方包。

**超时、取消与清理**
- 握手超时 10 秒，每次工具调用超时 30 秒，超时报 `E_DES_UNAVAILABLE`；
- 子进程设置 `kill_on_drop`，连接器被丢弃时子进程随之结束；
- 等待模拟工厂时不持有引擎锁；
- 每分钟先取齐所有仿真的数据，再一次性交给引擎并推进，中途失败时引擎状态不变（RT-DESC-04）。

**已知差异：** 刀具调拨失败时，远程模式在收到回执后才记补偿事件，事件顺序与进程内略有不同；这类故障场景不在 RT-DESC-01 的一致性范围内。

**外部内容进入 agent 上下文（7-fence-review.md 第 4 节）：** 模拟工厂返回的观测按 `Observation` 结构反序列化（只有数值、状态与编号字段），再写入 `world.des`；结构不符时报错，不写入。事件按 `EventInput` 结构入队，编号与批次必须带本仿真前缀。连接器不把模拟工厂返回的文本当作指令。
