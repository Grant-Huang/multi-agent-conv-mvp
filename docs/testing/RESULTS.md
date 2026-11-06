# 本次验证结果

## 代码与方案

开发前已执行 git fetch origin --prune 并快进同步到 `929301adf2de5403a832fdb17d9428d07ec06f58`。该远端比发布快照领先 12 个提交。

快照中的未提交修改没有被覆盖或重新应用：完整保存在 `stash@{0}`（说明 preserve published snapshot before verification sync），额外补丁与未跟踪文件归档位于 `/workspace/scratch/pre-sync-feature-verification/`。这些属于旧快照内容，本次不是将它们再次实现一遍。

新交付：审查设计、计划、8 个机器可校验 Feature 条目、6 条浏览器用例、地图校验器、真实服务 harness、统一 verify 命令、证据输出以及测试先行规范。未改 Rust 业务逻辑、任务表或公开 API；未修改 .github CI/权限策略。所有改动留在工作区供审查，没有推送、合并或发布。

## 本次实际命令与输出

### 地图校验器

先创建测试，未实现校验时 1 passed / 5 failed；重复 ID、缺路径、未知用例、缺浏览器测试及未登记测试均触发真实断言失败。实现后：

```text
ℹ tests 6
ℹ pass 6
ℹ fail 0
Feature map valid: 8 features, 6 browser cases
```

### 统一验证命令

在 web/、激活 Cargo 后执行 `npm run verify`，退出码 0；包含地图测试/检查、TS/Vite 构建、cargo build --locked --bin agentnexus、测试代码独立类型检查与 Playwright。实际浏览器输出：

```text
Running 6 tests using 1 worker
✓ E2E-CONV: 会话刷新和隔离
✓ E2E-FILE: 交付与真实文件下载
✓ E2E-APPROVE: 审批先于执行
✓ E2E-CANCEL: 两步确认取消
✓ E2E-DIAG: 开发者模式中的诊断与路由日志
✓ E2E-MOBILE: 窄屏会话抽屉
6 passed (6.0s)
```

真实 Chromium + Rust/HTTP/SSE/SQLite/文件工具；仅外部推理服务为受控替身，没有拦截应用 API。发现的测试夹具历史消息误判已修正，所有验收断言保留；每条用例显式建立独立会话。

截图、JSON/JUnit 和 HTML 报告分别位于 `web/test-results/` 与 `web/playwright-report/`；`web/test-results/runtime/` 含非空 backend.log、model.jsonl、state.json。单次数据目录由 state.json 指示，保留以便审查。报告输出被 Git 忽略，不假定随提交或发布保留。

### 全仓回归

`CARGO_TARGET_DIR=target cargo test --locked`：197 passed、0 failed、2 ignored。忽略的真实环境检查不算通过。当前是远端基线，不与旧快照中额外未提交功能的测试数量混算。

`CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings`：退出码 0，无警告；实际末行：

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.89s
```

`CARGO_TARGET_DIR=target cargo fmt --all -- --check`：退出码 0，无差异。

`cd web && npm test`：

```text
Test Files  4 passed (4)
Tests  17 passed (17)
```

`npm run verify` 包含的 `npm run build` 通过，实际输出含 `✓ 278 modules transformed.` 与构建完成；浏览器 spec/config 另外经过 tsc --noEmit -p e2e/tsconfig.json。

### 资源清理

本次后端 PID 已退出，127.0.0.1:18787 已关闭，慢模型请求记录 slow-disconnected。只清理本次服务；没有删除用户数据库、工作文件或其他服务。运行日志和测试数据仍保留。

## 可重复运行与限制

重复运行 npm run verify 已通过，使用新的临时数据目录且不复用旧后端。未执行真实模型质量/真实搜索/真实多机节点/长压测，不将浏览器功能测试等同于这些验收。M4.3 与 MOM 仍按远端设计状态列为未实现，不在本次范围。

浏览器优先显式选择的路径或系统 Chromium；本次用 /usr/bin/chromium。其他机器若下载 Playwright 浏览器，需要相应网络许可与系统依赖。没有更改网络策略或关闭 TLS。

新的 CI 接入尚未实施；当前提供本地与 CI 均可调用的 verify 命令，未声称远端 CI 已运行。AGENTS.md 属治理文件，后续合并仍遵守既有 CODEOWNERS 与风险审查，本次不自行批准。

## 推送前复验

提交前发现远端又新增 5 个提交，已将本次分支 rebase 到 `20c0153`，无冲突。再次运行全仓 Rust：212 passed、0 failed、2 ignored；严格 Clippy `-D warnings` 与 cargo fmt --check 均退出 0。前端 17 项、地图 6 项与浏览器 6 项再次通过（浏览器实际输出 `6 passed (6.2s)`）。以上初次基线记录保留为历史证据，本节是推送版本的复验结果。

## MOM 后续实现与验证

后续按用户授权完成独立 MOM A 层模拟实现，已不属于“未实现”功能。地图增加 mom、浏览器增加 E2E-MOM/E2E-COMPLAINT；全量 `npm run verify` 8 项通过。模型与工业系统仍为替代件，详见 [MOM 验证记录](../mom/RESULTS.md)。原验证记录保留为历史，不将本次 Mock 结果解读为真实生产能力。

## M4.3 运行时用例（RT-TPL-01 ~ RT-TPL-10）

### 诚实说明

M4.3 的确定性校验实现（`crates/runtime/src/template.rs`、`crates/llm/src/backend.rs` 的 `background_capacity`）**先于**本次用例编写。为补上 Red→Green，本次做了以下事情：

1. 备份实现（sha256 `00b2f70c…`），用例定稿后，先把实现替换为**桩**（签名不变，逻辑为占位），跑用例看业务断言失败（Red-1）；
2. 再用**类型变异**单独验证由类型保证的规则（Red-2）；
3. 恢复备份实现，全部通过（Green）。

RT-TPL-08 的缺失场景在 Red 之前修正过一次：原场景的 payload 同时多出字段又缺字段，断言期望的原因文字与实现报告的顺序不一致。改为 CASES 描述的"字段不全"（payload 为空），断言改为具体的缺失字段。这次修正发生在 Red 之前，不是为了迎合实现。

### Red-1（实现替换为桩）

```
test rt_tpl_01_quote_template_loads ... ok
test rt_tpl_03_dependency_edges_are_rejected ... ok
test rt_tpl_02_capability_is_a_router_role ... ok
test rt_tpl_07_payload_values_must_be_strings ... ok
test rt_tpl_04_branches_must_fit_background_capacity ... FAILED
test rt_tpl_05_unclear_router_slots_stay_single ... FAILED
test rt_tpl_06_envelope_fields_must_match ... FAILED
test rt_tpl_09_join_rejects_unknown_or_duplicate ... FAILED
test rt_tpl_08_missing_branches_are_not_filled ... FAILED
test rt_tpl_08_all_branches_ok_completes_join ... FAILED
test result: FAILED. 4 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
test rt_tpl_10_capacity_follows_backend_config ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
```

失败的是 RT-TPL-04、05、06、08（两条）、09 与 RT-TPL-10 的业务断言。RT-TPL-01、02、03、07 在桩阶段仍通过：前两条是正向用例；02、03、07 由类型声明保证，由 Red-2 验证。

### Red-2（类型变异，逻辑保持真实）

```
test rt_tpl_04_branches_must_fit_background_capacity ... ok
test rt_tpl_01_quote_template_loads ... ok
test rt_tpl_06_envelope_fields_must_match ... ok
test rt_tpl_05_unclear_router_slots_stay_single ... ok
test rt_tpl_08_all_branches_ok_completes_join ... ok
test rt_tpl_08_missing_branches_are_not_filled ... ok
test rt_tpl_09_join_rejects_unknown_or_duplicate ... ok
test rt_tpl_02_capability_is_a_router_role ... FAILED
test rt_tpl_03_dependency_edges_are_rejected ... FAILED
test rt_tpl_07_payload_values_must_be_strings ... FAILED
test result: FAILED. 7 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
error: test failed, to rerun pass `-p an-runtime --test template_cases`
```

失败的是 RT-TPL-02（能力名）、03（依赖边）、07（payload 类型）。变异版本为编译通过，只调整了 payload 的 trim 检查与空映射构造，业务逻辑未变。

### Green（恢复实现）

```
test rt_tpl_01_quote_template_loads ... ok
test rt_tpl_04_branches_must_fit_background_capacity ... ok
test rt_tpl_03_dependency_edges_are_rejected ... ok
test rt_tpl_02_capability_is_a_router_role ... ok
test rt_tpl_05_unclear_router_slots_stay_single ... ok
test rt_tpl_06_envelope_fields_must_match ... ok
test rt_tpl_07_payload_values_must_be_strings ... ok
test rt_tpl_08_all_branches_ok_completes_join ... ok
test rt_tpl_09_join_rejects_unknown_or_duplicate ... ok
test rt_tpl_08_missing_branches_are_not_filled ... ok
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test rt_tpl_10_capacity_follows_backend_config ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 校验器的 Red→Green

`web/verification/check-map.test.mjs` 新增 4 个 runtime 映射测试：扩展前 3 条失败（未知用例、缺测试、未映射测试），扩展后 `node --test` 为 10 通过、0 失败。

## 本次验证命令与结果

- `npm run test:map`：`Feature map valid: 9 features, 6 browser cases`
- `cargo fmt --all -- --check`：通过
- `CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings`：退出码 0
- `CARGO_TARGET_DIR=target cargo test --locked --workspace`：260 passed，0 failed，2 ignored
- `cd web && npm test`：17 passed
- `cd web && npm run build`：通过
- `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/opt/pw-browsers/chromium npm run verify`：退出码 0，6 个浏览器用例全部通过（`6 passed (8.3s)`）

环境记录：首次 verify 失败是因为 Playwright 默认的 headless shell 路径不存在，改用 `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` 后通过；之前 `npm ci` 因磁盘写满未完整安装 `@types/node`，重新安装后解决。

范围限制：M4.3 的并行执行与 Composer 汇合回复未接入编排器，RT 用例不覆盖它们；M4.4（真实模型 F 组覆盖率）未运行。
