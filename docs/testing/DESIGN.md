# Feature Map 与端到端验证体系设计

## 审查与范围

基线：同步后的 origin/master 929301a。同步前发布快照的未提交改动已完整保留在 Git stash 和 /workspace/scratch/pre-sync-feature-verification/，未重新叠加。本次以远端真实实现为准。

原方案仅有静态指南、HTTP/Runtime/组件测试，不能证明真实 UI 可用；功能索引没有自动检查文档/测试失配；缺少标准化启动、证据与清理。远端已将诊断/路由日志藏在开发者模式，取消使用三秒内两次点击，旧操作路径过时。M4.3 模板及 MOM 仍是设计，不能称为现有功能。CI 已存在，不能声称需要从零建立门禁。

本次交付：可校验的地图、真实浏览器关键路径、单一验证命令与证据。不恢复旧 M4.3 实现，不开发 MOM，不引入自动发布/合并。

## 结构

- docs/testing/features.json：稳定 Feature ID、Case ID、文档/实现/测试位置、用户路径、预期结果、覆盖层级。
- docs/testing/FEATURE_MAP.md：当前功能入口和边界，面向操作与排错。
- docs/testing/CASES.md：在实现验证 harness 前定稿的浏览器验收用例。
- web/verification/check-map.mjs：检查重复/未知 ID、路径缺失、用例没有测试、测试未登记等；不把“文件存在”当作功能已通过。
- web/e2e/*.spec.ts：Playwright Chromium 测试，走 UI→HTTP/SSE→Rust Runtime→SQLite/文件。仅 OpenAI 兼容模型端点用本机假服务。
- web/e2e/harness.mjs：启动专用模型服务和真实 agentnexus 二进制，配置、数据库、文件、日志均位于单次临时目录；进程退出前保存日志。
- npm run verify：地图校验、前端构建、后端构建、浏览器用例。单元/全仓回归另行运行，不能把此命令当所有质量门禁。

## 浏览器范围与断言

首版六条：会话刷新/隔离；文档任务交付及下载内容；审批前无执行并由 UI 批准；两步取消；开发者模式诊断和路由日志；移动端会话抽屉。全部使用真实应用，不 route.fulfill 应用 API。

断言同时覆盖可见内容和对应 API/产物；SSE 用浏览器收取，回答不得泄漏 think 内容。模型替身只服务固定测试问题，不代表真实模型准确率。节点实机、真实搜索、剪贴板权限和长时间并发压测明确保留待补。

## 隔离、期限与证据

浏览器用全新 context；测试会话独立 ID。后端仅监听 127.0.0.1:18787，Playwright 不复用已有服务；模型端口动态分配。真实 coding、ACP 自动发现、节点发现、联网搜索禁用。测试最长 30 秒，单个断言最长 10 秒，服务启动最长 30 秒；构建由验证命令先完成。

SIGINT/SIGTERM 时仅终止本次子进程，五秒后必要时升级 SIGKILL；关闭模型 sockets，保存应用日志及临时目录位置。临时数据不自动删除，以保留失败证据；用户可按记录清理本次目录。取消用例的慢模型在连接断开时清除计时器。

输出：web/test-results/（JSON、JUnit、截图/失败 trace）、web/playwright-report/（HTML）、web/test-results/runtime/（后端/模型日志和数据位置）。无证据则未验证；进程 PID、HTTP 200、mock 分数不能替代断言。

## 开发与维护契约

先写 CASES 和可执行测试，再实现 harness/校验器；现有产品行为属于基线测试，不冒称产品代码的 Red→Green。校验器用非法地图先得到业务断言 Red，再实现 Green。变更功能路径时同改 manifest、文档与测试。不会自动改验收条件或把失败改成跳过。

不修改现有 CI/权限策略；提供可被 CI 调用的 npm run verify。CI 接入仍需按仓库 L3 规则审阅，不把本地通过描述为远端 CI 已通过。浏览器优先使用显式 PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH 或已有 /usr/bin/chromium；其他机器用 Playwright 安装匹配 Chromium，保持 TLS 校验。
