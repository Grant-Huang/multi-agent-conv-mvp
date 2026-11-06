# 测试指南

先读 [功能地图](FEATURE_MAP.md)、[方案审查与设计](DESIGN.md)、[端到端用例](CASES.md)，进度见 [计划](PLAN.md)。当前远端代码已与原快照不同，地图不把旧的未提交实现当作当前版本。

## 一次可重复的验证

需要 Rust/Cargo、Node 22+、npm、Chromium。云环境激活示例：

```bash
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH="/workspace/.cargo/bin:$PATH"
cd web
npm ci
# 已有 /usr/bin/chromium 时自动使用；也可显式选择：
# export PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/path/to/chromium
# 其他机器：npx playwright install chromium
npm run test:map
npm run verify
```

verify 顺序：地图检查→Vite/TS构建→Cargo build（CARGO_TARGET_DIR=target）→真实 Chromium 用例。占用专用 loopback 18787，不复用现有服务器；如端口占用先定位，不终止不属于本次的服务。所有业务数据在一次性临时目录，状态记录在 web/test-results/runtime/state.json；证据先保留，不自动删除目录。

只执行已构建的浏览器测试可用 npm run test:e2e。无需真实模型密钥、搜索或 ACP 安装；外部模型是受控替身，应用/HTTP/SSE/SQLite/文件工具真实运行。

## 证据与判定

web/test-results/results.json、junit.xml、cases 下截图/失败 trace，playwright-report 下 HTML。runtime 下保存后端日志、模型事件和临时数据位置。HTML 可用 npx playwright show-report 在本机查看；不为云 onboarding 制造 localhost 预览链接。

每次记录代码版本和 diff、Case/Feature ID、命令/退出码、passed/failed/skipped、证据路径和清理结果；没有运行就是未验证。模型被替换，所以通过不能解释为真实模型质量达标。

## 其他检查与开发顺序

仓库根执行 CARGO_TARGET_DIR=target cargo test --locked、cargo clippy --locked --all-targets、cargo fmt --all -- --check；web 内 npm test、npm run build。保留现有 CI 门禁，不修改它们，也不声称本地通过已等于远端 CI 通过。

新增/改变功能先写 CASES 与可执行 E2E，再改实现；测试定稿后不为迎合实现改断言或跳过。不同层级分别记录：浏览器 E2E、HTTP E2E、Runtime 集成、组件/单元、真实外部依赖。校验器测试会故意输入非法地图确认能失败。

方案实施与本次实测结果见 RESULTS.md；持续维护时重复 npm run test:map 和相关浏览器用例。
