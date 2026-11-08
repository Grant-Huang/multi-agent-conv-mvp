# R-FIRST-05 修复证据（2026-11-08）

基线：origin/master e0811a2。任务来源：docs/review/2026-11-07.md 交接清单第 3 项与台账 open 条目。

## 范围与验收

只在 conversation.rs 内移动补充要求的取消时机：Router 与 Policy 成功后，才调用现有 cancel/submit。直接委派和单独取消入口保持原行为；不改 HTTP、config.toml、AgentAdapter、状态机/持久化高风险文件或权限策略。

实现前定义 RT-CONV-01/02，并新增 `crates/server/tests/supplement_routing.rs`。这是受控模型 + 真正 Runtime/Router/Policy/SQLite 的集成验证，不是 HTTP 或浏览器 E2E。旧任务设为 NEEDS_HUMAN，避免测试实际执行文件操作；失败时比较完整任务对象和事件列表，且任务数量仍为 1。成功时目标包含原要求和补充要求，只路由一次，旧任务取消，新任务待审批。

## Red

先只新增验收和测试，未改业务实现：

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test supplement_routing
rt_conv_01_failed_supplement_preserves_existing_task ... FAILED
rt_conv_02_successful_supplement_replaces_task_after_routing ... ok
assertion `left == right` failed: 路由失败不能取消旧任务：transport
实际任务状态 CANCELLED，期望 NEEDS_HUMAN（完整任务对象断言）
test result: FAILED. 1 passed; 1 failed; 0 ignored
exit=101
```

失败测试在首个 transport 场景停止；未把后续 format/policy 场景说成原实现上的独立 Red。成功路径是基线保护，不追称新行为 Red。

## Green

实现后原测试/断言不变：

```text
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test supplement_routing
rt_conv_02_successful_supplement_replaces_task_after_routing ... ok
rt_conv_01_failed_supplement_preserves_existing_task ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; finished in 0.03s
exit=0
```

失败回归依次覆盖传输 503、无效 JSON、Policy 拒绝无可用 coding agent；成功回归保护正常替换。不是按输入或错误文本特判，三类失败共用 route 的错误分支。

## 完整检查实际输出

为控制环境磁盘使用，Rust 设置 CARGO_INCREMENTAL=0、CARGO_PROFILE_DEV_DEBUG=0、CARGO_PROFILE_TEST_DEBUG=0，CARGO_HOME=/workspace/.cargo、RUSTUP_HOME=/workspace/.rustup；不跳过测试或关闭警告。target 使用已有可再生编译缓存。npm 缓存位于 /workspace/.npm-cache。

```text
CARGO_TARGET_DIR=target cargo fmt --all -- --check
exit=0
CARGO_TARGET_DIR=target cargo test --locked
exit=0；所有 test result 汇总：387 passed, 0 failed, 2 ignored（原有真实环境测试）
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings
Finished `dev` profile [unoptimized] target(s) in 10.69s；exit=0，无 warning/error
cd web && npm ci && npm test && npm run build
全部 exit=0；Test Files 7 passed (7)；Tests 21 passed (21)；✓ built in 386ms
cd web && npm run test:map
Feature map valid: 14 features, 14 browser cases；exit=0
git diff --check
exit=0
```

## 异步、取消、清理与局限

路由请求沿用现有 LLM 超时、重试和会话锁；等待 Router 时不再取消旧任务，调用方在该等待阶段取消 future 也不会先取消旧任务。成功后调用原有 orchestrator.cancel/submit，不创建新线程或业务后台进程。测试整体请求期限 10 秒、LLM 期限 5 秒、无重试；测试结束中止并等待本测试启动的模型服务器，删除本测试临时目录。

本轮未验证真实运行中的 agent、浏览器 UI 或真实模型。取消旧任务与写入新任务仍非原子操作，submit/持久化失败时旧任务可能已取消；要进一步保证原子替换将涉及 orchestrator/store 高风险文件与方案审批，本轮不扩展修复范围，也不声称解决该窗口。同会话用户主动取消与完成竞态沿用现有状态机保护。

本记录只支持 fix_claimed，问题是否关闭由评审 agent 在 master 独立核对。
