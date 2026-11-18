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

---

# AG.5 第 3、4 层（诊断包、请其他助手帮忙）证据

日期：2026-11-18。用户批准：D1 接口约定定稿（9 项改动 + 2 处缺口）；没配置 DLP 时第 3 层诊断包照样显示并注明；第 4 层这次不做"修改请求转确认卡"和"从对话发起、结论写回会话"（docs/DECISIONS.md 第 12～14、17 条）。

## Red → Green

- `cargo test -p an-server --test agent_diagnose_send`：
  - 实现前（提交 03d47f4、26783f6）：`1 passed; 12 failed`。失败都是业务断言：排查结果里没有诊断包，`/api/agents/diagnose/preview` 和 `/send` 返回 404。通过的那条是守护用例 RT-DLP-01。
  - 实现后：`13 passed`。
- 浏览器 E2E-AGHELP（Red 提交时编号为 E2E-AGDIAG2，后来发现功能地图只认纯字母编号，所以改了名，内容未变）：
  - 实现前：失败在找不到「诊断包」区域，其余 10 条 agents 用例通过。
  - 实现后：全部浏览器用例 `25 passed`。

如实记录一处实现修正（断言未改）：RT-AGDIAG-05 第一次实现后失败。原因是这台机器的主机名是 `vm`，只有 2 个字符，而实现规定"少于 3 个字符不替换"，所以主机名没被替换。修正后，2 个字符的名字按整词替换（前后都不是字母、数字、`_`、`-`），1 个字符的不替换；并补了单元测试。

## 改动

- `an-agents/src/dlp.rs`（新）：DLP 客户端。复用 MCP 网关（输入、输出都按 schema 校验），校验统一外壳、`status` 和 `api_version`；任何失败一律当作"不能发"；连接按需建立，失败后丢掉，下次重连。
- `an-agents/src/diagnose.rs`：诊断包组装 `package_text` 和本机确定性处理 `scrub_local`（家目录、用户名、主机名替换；环境变量只列名字）。
- `an-core`：`DiagnosePackage`、`DiagnosePreview`、`DiagnoseSendResult`；`DiagnoseReport` 增加 `package`。已重新生成 TS 类型。
- `[dlp]` 配置（`AppConfig.dlp`）；`Runtime.dlp`、`Runtime.diagnose_send`。
- `an-runtime/src/agent_diagnose.rs`（新）：第 3 层诊断包；第 4 层预览和发送（同意凭证、全局闸门、帮手临时实例、还原、撤销）。
- `POST /api/agents/diagnose/preview`、`POST /api/agents/diagnose/send`；排查卡增加诊断包、选择帮手、外发预览、帮手回复。
- `config.example.toml`：DLP 接入示例改为 `[dlp]`，原来放在 `[[mcp.servers]]` 会把工具暴露给模型。
- 测试工具：模拟 DLP 服务 `crates/server/tests/fixtures/dlp_server.py`；`an-mock-acp` 新增 `AN_MOCK_ACP_RECORD`、`AN_MOCK_ACP_REPLY`。

## 超时、取消与清理

- DLP：每次调用最多 `timeout_secs` 秒（默认 5 秒），超时就当作不可用、不发送；DLP 子进程随连接释放而结束。
- 帮手：
  - 单次最多 120 秒（超时返回 504）。超时或请求被取消时，执行的 future 被丢弃，ACP SDK 结束帮手的整个进程组（RT-AGDIAG-10 确认进程已结束）。
  - 临时工作目录在结束时删除，请求被取消时由 Drop 删除（RT-AGDIAG-08）。
  - 全局同时只运行 1 个，第二个请求返回 409，凭证保留（RT-AGDIAG-11）。
- 同意凭证：5 分钟过期，只能用一次；过期的凭证在下次预览时清理，并撤销对应的映射。
- 撤销：发送成功、失败、超时都会撤销映射（RT-DLP-06、RT-AGDIAG-10）；第 3 层诊断包用完立即撤销。

## 完整检查（本轮实际输出）

- `cargo fmt --all -- --check`：exit=0
- `cargo test --locked --no-fail-fast`：passed=535 failed=0 ignored=2
- `cargo clippy --locked --all-targets -- -D warnings`：exit=0（工具链 1.100，与 CI 相同）
- `cd web && npm test`：21 passed；`npm run build`：✓ built；`npm run test:map`：26 features, 25 browser cases
- `npx playwright test --trace off`：25 passed

## 未覆盖

- 只用模拟的 DLP 服务测过。真实的 `tools/dlp-egress` 要等工具一侧按 CONTRACT 第 8 节改完，包括结构化返回、`api_version`、`channel` 增加 `agent` 和 `clipboard`、新增实体类型，以及修复审查发现的 H1、M2～M5，再做一次接入验收。
- 只用模拟的帮手测过。真实的 Claude Code 等助手在"权限一律拒绝"下会怎么回复，需要在本机验收（AG.6 的 J8）。
- 错误详情里 SDK 的报错文本和附加数据有重复（AG.1 起就是这种写法），诊断包里会出现两次，不影响判断。
