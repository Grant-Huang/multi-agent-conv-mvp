# World Adapter M1-1 Green — `/api/world/des`

分支：`cursor/wa-m1-1-green-des-f2df`。未改 `DesConnector` / `engine.rs`。

## 实现

- `crates/server/src/mom_world_des.rs`：`/api/world/des` 绑定 / Observe / Subscribe / trial / official / step
- `main.rs`：与 `world_mock` 同门控 merge；`WorldDesState::new(true)`
- 工作台：`data-provenance`、水位 / 续读 / 过期游标；事项 `formal-observation`
- DES 实验页：`adapter-mode[data-mode=des]`
- 测试：`crates/server/tests/mom_world_des.rs` + `features.json` `RT-WADES-*`

## Green

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture
test result: ok. 4 passed; 0 failed
```

```text
$ cd web && npm run test:map
# Feature map valid
```

Playwright：

```text
$ PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/local/bin/google-chrome npx playwright test e2e/wa-des-remote.spec.ts
3 passed (2.3s)
```

## 非目标

未接真实 DesConnector 子进程；写路径仍 Mock；未改 engine。
