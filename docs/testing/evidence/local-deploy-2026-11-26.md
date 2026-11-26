# 本地可部署性证据（W1 / #424）— 进行中

- **日期**：2026-11-26 America/Vancouver
- **线**：第 6 线
- **基线**：`origin/master` @ `300f9b7`（工作分支 `cursor/line6-w1-deployability`）

## D2 配置完整性（本 PR）

对照 `AppConfig`（`crates/runtime/src/config.rs`）用户可配节，`config.example.toml` 已补：

| 节 | 默认（代码） | 样例 |
|---|---|---|
| `[desktop]` | `test_fixtures=false` | 已加注释块 |
| `[mom]` | `chat_bridge=false` 等 | 已加注释块 |
| `[introspection]` | `enabled=false` 等 | 已加注释块 |
| `[conversation]` 资料注入 | `session_materials=true` 等 | 已加注释说明 |

另：`README.md` 快速开始补充 AI 桌面 / MOM / 记忆 / 内省开启指引。

验证：含三节的最小 `config.toml` 可被当前 `agentnexus` 解析并启动到 `server.started`（本机临时端口自检）。

## 待做（同卡后续提交）

- D1 冷启动耗时与卡点记录
- D3/D4 真实模型与本机 agent（可与 #419 交叉）
- D5～D7 升级 / 回滚 / 备份（依赖 #425 W2 数据集；未就绪前用接口手工造数）
