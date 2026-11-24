# 证据：T-88 首页与旧壳整治（A1 欠账清偿 + 生产界面去测试控制）

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户发号 T-88 并批准「一次到位：连旧壳一起拆」（对话）
- **背景**：用户指出首页布局混乱——应在后台配置的内容跑到前台、元素过多、注意力被非重点功能分散。截图核对确认三根因：① A1「一次换壳」欠账（旧顶栏页签/右侧四-tab/agent 诊断列表仍在）；② 配置管理类内容进前台（助手管理引导卡、MOM/DES/运营链接行、未启用横幅）；③ 测试/调试面板泄漏（注入按钮、已读重置夹具）。

## 改动

| 区域 | 改动 | 依据 |
|---|---|---|
| `App.tsx` | 拆旧顶栏 `.main-tabs`；右栏默认改为助理消息面板（legacy 页签仅会话视图保留——助手管理入口保留）；侧栏加「任务」「运营」分区（运营工作台/频道按权限显示，未启用=禁用态）；发现卡迁设置-连接与能力；HealthBox 诊断行默认折叠 | 设计原则 §2（不按执行主体组织导航）/§13（诊断折叠）；DESIGN §2；GAPS §5.4 A1 |
| `DesktopHome.tsx` | 删配置链接行/未启用横幅/lead 文案 | 元素过多治理 |
| `S1AskBar.tsx` | 删 label 与解释 hint（保留输入+问答/委派） | 同上 |
| `AttentionToast.tsx` | 移除注入按钮（测试控制）；列表空不渲染；保留忽略/静音（正式用户控制，DESIGN §7） | MOCK_REGISTRY 规则 2 |
| `AssistantReadPanel.tsx` | 移除「重置夹具/刷新」调试按钮；加 5s 轮询（跨设备已读同步）；标题改「助理消息」 | 同上 |
| `SettingsPanel.tsx` | 「连接与能力」分类渲染发现卡（首次）+ 助手管理列表（AgentsPanel） | 用户指示「连接多 agent 功能保留，放到合适的地方」 |
| E2E | ADATTN 注入改 API+轮询；ADXR 刷新按钮改轮询；AGFIRST 入口改设置路径；新增 E2E-ADHOME（UI16） | 断言零放松，观察路径适配 |

## 验证

- typecheck ✓ / build ✓ / vitest 31 ✓（health.test 2 例按折叠后行为适配：先展开「连接详情」再断言圆点与原因，断言强度不降）
- `desktop.spec` 32/32（31+ADHOME）（fresh build；含新增 E2E-ADHOME）
- 全量 E2E（10 套件）与 workspace：见 PR 正文（rt_adm_14 间歇失败已单独立证移交）
- 新增用例 RT-ADHOME（CASES 已登记）

## 非目标

- 旧 ChatView 会话流保留（他线验收依赖：agents/features/codeflow/session-materials/restart 套件）
- 多用户角色系统（运营入口暂以 opsUnread 门控）
- MOM 工作台内部布局（归 MOM 线）
