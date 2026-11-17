# AG.5 第 1、2 层（规则库、本机体检）证据

日期：2026-11-17。用户批准：第 1、2 层的验收用例；本机体检用登录 shell 运行写死的脚本打印 PATH（只对目录预设命令做）；`node --version` 与凭证文件存在性检查（只对目录里注明需要的助手做，不读取内容，每条 5 秒超时）。第 3 层（诊断包）、第 4 层（外部助手排查）改为经 DLP，等 D1 定稿。

## Red → Green

- `cargo test -p an-server --test agent_diagnose`：实现前 `0 passed; 5 failed`，均为接口不存在（404）等业务断言。实现后 `5 passed`。
- 浏览器 E2E-AGDIAG：实现前找不到「排查」按钮；实现后全部浏览器用例 `24 passed`。

如实记录一处测试搭建修正（断言未改）：RT-AGDIAG-03 的"node 卡住"假脚本原写 `sleep 30`，但用例期间 PATH 只指向临时目录，`sleep` 找不到、脚本立刻退出，超时路径没有被测到（整个文件 0.08 秒跑完）。改为 `exec /bin/sleep 30` 后，该文件耗时 5.09 秒，确认走到了 5 秒超时并记为"检查失败"。

## 改动

- `an-core`：`CheckItem`、`Finding`、`DiagnoseReport`（已生成 TS 类型）。
- `an-agents/src/catalog.rs`：每个预设增加 `node_min_major`（Claude Code、Codex 适配器需要 Node；ZCode 桥接需要 ≥ 22）和 `credential_files`（ZCode：`~/.zcode/v2/config.json`）。
- `an-agents/src/diagnose.rs`（新）：本机体检与规则库。规则：`path_mismatch`、`not_installed`、`node_missing`、`node_too_old`、`credential_missing`、`needs_login`、`timeout`；检测成功时不给结论。
- `Runtime::diagnose_agent`、`POST /api/agents/diagnose`；「助手」页签每行"排查"和排查结果卡。

## 边界

- 只运行写死的命令：`$SHELL -lc "printf '%s' \"$PATH\""`、`<node> --version`。命令名不经过 shell，在登录 shell 打印出的 PATH 里由代码查找；用户自定义命令不做登录 shell 检查（命令与目录预设不一致时也算自定义）。
- 凭证文件用 `symlink_metadata` 只读元数据，不打开文件（RT-AGDIAG-04：文件权限 000 时仍判为存在，结果里没有文件内容）。
- 超时与取消：每条 5 秒；超时先 SIGTERM 再 SIGKILL 整个进程组，请求被取消时立即 SIGKILL（`proc_group`）。
- 不外发；未命中规则时日志只记分类和错误输出长度。

未覆盖：真实助手的错误文字（规则特征可能需要按用户本机实测补充）；Windows（没有进程组）。

## 完整检查（本轮实际输出）

- `cargo fmt --all -- --check`：exit=0
- `cargo test --locked --no-fail-fast`：exit=0，passed=472 failed=0 ignored=2
- `cargo clippy --locked --all-targets -- -D warnings`：exit=0，无警告
- `cd web && npm test`：Tests 21 passed (21)
- `npm run build`：✓ built
- `npm run test:map`：Feature map valid: 24 features, 24 browser cases
- `npx playwright test --trace off`：24 passed (21.4s)
