# 模拟工厂接入 an-mom 阶段⑤a-3 Red → Green 证据

分支：`claude/relaxed-fermi-u38fye`（master 632933a 之上）。设计：[DES_CLOSED_LOOP.md](../../mom/DES_CLOSED_LOOP.md) §11a"⑤a-3 设计"；决策：[DECISIONS.md](../../DECISIONS.md) DES-28（助手建议，待用户确认）；用例：[CASES.md](../CASES.md)「阶段⑤a-3：an-mom 接入模拟工厂连接器」。

范围：本提交只包含以下内容，行为尚未实现：
- 用例 RT-DESC-06～12；
- 设计与决策；
- 骨架：
  - `AppState.des` 字段与 `AppState::with_des`；
  - `mom::clock_step`：只有原来的进程内行为，后台时钟改为调用它；
- 失败测试：
  - `crates/server/tests/mom_des_http_mcp.rs`（RT-DESC-06～11）；
  - `crates/server/tests/mom_des_connector.rs` 末尾（RT-DESC-12）。

`an-des-mcp` 可执行文件的查找函数移到 `crates/server/tests/support/des_mcp.rs`，两个测试文件共用。函数内容不变，已有测试断言没有改动。

## Red

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_http_mcp --test mom_des_connector --no-fail-fast

mom_des_connector：5 passed; 1 failed
  rt_desc_12_a_new_factory_ignores_old_simulations_and_keeps_ids_unique
    old simulations do not block dispatch: factory rejected maintenance_order_create: unknown simulation DES-1

mom_des_http_mcp：0 passed; 6 failed
  rt_desc_06_an_mom_starts_with_the_simulated_factory_or_exits    help documents --des-mcp
  rt_desc_07_http_create_and_advance_match_the_in_process_service  left: Null  right: "in_process"
  rt_desc_08_http_execution_sends_writes_before_returning          left: Null  right: "mcp"
  rt_desc_09_the_shared_clock_advances_through_the_factory         left: Null  right: "mcp"
  rt_desc_10_offline_factory_returns_503_without_fallback          left: 200   right: 503（创建仍走进程内仿真）
  rt_desc_11_desktop_tools_and_views_go_through_the_factory        left: 404   right: 400（工具接口尚不存在）
```

失败都发生在业务断言上，不是环境问题：
- `an-des-mcp` 与 `an-mom` 都已构建；
- 模拟工厂正常启动；
- RT-DESC-01～05 仍然通过。

## 其他检查（同一提交）

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --locked -p an-server --all-targets`：无警告。
- `cd web && npm run test:map`：`Feature map valid: 28 features, 25 browser cases`。

## Green（2026-11-19，用户确认用例与 DES-28 之后）

实现：
- `crates/des/src/service.rs`：`simulation_create` 支持可选的 `simulation_id`。
- `crates/runtime`：
  - `des_next_simulation_id`、`des_is_remote`；
  - `des_whatif` 对远程仿真读快照；
  - `playback_tick` 拆成两段（行为不变）。
- `crates/server/src/mom_des_connector.rs`：串行锁；创建时传入编号；只发自己创建的仿真的写操作。
- `crates/server/src/mom.rs`：
  - `advance` 命令与后台时钟经连接器推进；
  - 命令之后立即发写操作；
  - 离线时返回 503，正文 `code` 为 `E_DES_UNAVAILABLE`。
- `crates/server/src/mom_sim.rs`：
  - 创建接口返回 `source`；
  - 远程仿真的工厂视图、趋势预测转调观测工具；
  - 新增工具接口。
- `crates/server/src/bin/an-mom.rs`：新增 `--des-mcp` / `AN_MOM_DES_MCP`；管理口令随机生成；启动行打印实际地址。

测试改动（如实记录）：RT-DESC-09 的前置条件补了"先取得 S1 案例"，断言未改。原因是仿真起点为 497 分钟，用例从 0 分钟开始；按 DES-27，起点之前模拟工厂没有时钟，断言不可能成立。见 DECISIONS DES-28 补记。

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_http_mcp --test mom_des_connector --test mom_des --no-fail-fast
mom_des            test result: ok. 5 passed; 0 failed
mom_des_connector  test result: ok. 6 passed; 0 failed   （RT-DESC-01～05、12）
mom_des_http_mcp   test result: ok. 6 passed; 0 failed   （RT-DESC-06～11）
```

完整检查：
- `cargo fmt --all -- --check`：通过。
- `CARGO_TARGET_DIR=target cargo test --locked --workspace --no-fail-fast`：退出码 0；63 个测试目标全部 `ok`，共 601 个通过、0 个失败。
- `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets`：退出码 0，无警告。
- `cd web && npm run build`：`✓ built in 1.81s`。
- `npm run test:map`：`Feature map valid: 28 features, 25 browser cases`。

超时、取消与清理：
- 沿用连接器的握手 10 秒、每次调用 30 秒超时；
- `advance` 每分钟原子提交，请求被取消时只丢失未完成的那一分钟；
- an-mom 退出时，模拟工厂子进程随连接器结束（`kill_on_drop`）；
- 后台时钟出错时暂停播放并记录日志。
