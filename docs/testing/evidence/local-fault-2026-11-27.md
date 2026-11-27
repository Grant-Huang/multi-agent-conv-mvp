# W5 故障与恢复首轮（F1～F3）— 2026-11-27

| 项 | 值 |
|---|---|
| Issue | #427 |
| 日期 | 2026-11-27（America/Vancouver） |
| 二进制 worktree commit | `b113aeb`（fault 实验室） |
| 证据落库基线 | `origin/master` @ `e3deca8`（含 #524） |
| 测口 | `http://127.0.0.1:18787`（专用故障实验室，与 8787 隔离） |
| OS | macOS 26.6.2 (25G83) |
| 模型 | llama.cpp `:8200` → `Qwen3.8-35B-A3B-Q4_K_M.gguf` |
| 本机 agent | Codex CLI `0.166.0`（F3）；F1 委派多为 builtin `general` |
| 配置 | `/tmp/line6-d1-clean/repo/config.fault.toml`（`bind=127.0.0.1:18787`，`db=data/fault.db`） |
| 原始结果 | `/tmp/line6-fault/evidence/f123-results.json`、`f1-rerun.json` |

## 汇总

| 用例 | 次数 | 通过 | 说明 |
|---|---|---|---|
| F1 kill -9 后端 | 3 | **3/3** | 任务 → `FAILED`；会话消息保留（≥3）；重启后 health=200。首轮 harness 曾因僵尸 pid 误判 1/3，补跑后 3/3（见下） |
| F2 模型中断 | 3 | **3/3** | `base_url` 临时改 `127.0.0.1:9` → 错误/可重试提示；恢复 8200 后同会话可继续 |
| F3 杀本机 agent | 3 | **3/3** | 路由到 coding/`codex` 后 `SIGKILL` 新出现的 agent PID；任务 `FAILED`；被杀 PID 无残留 |

## F1

注入：委派 `write_file` 任务进入 `RUNNING` 后对后端 `kill -9`，再启动。

| # | task_id（短） | kill 前 | 重启后 | 消息数 | 判定 |
|---|---|---|---|---|---|
| 1 | …a396e472 / 补跑 | RUNNING | FAILED | 3 | 通过 |
| 2 | 补跑 | RUNNING | FAILED | 3 | 通过 |
| 3 | 补跑 | RUNNING | FAILED | 3 | 通过 |

期望对照（LOCAL_E2E §6）：任务状态可解释（恢复或明确失败）✓；会话不丢 ✓。

首轮 `f123-results.json` 中 #2/#3 业务结果同为 `FAILED`+消息保留，但 harness 在 macOS 上于 `SIGKILL` 后短暂仍探测到 pid（疑僵尸），将 `pass` 标假；补跑将探测改为忽略 `Z` 态后 3/3。

## F2

注入：把 `[llm].base_url` 改为不可达 `http://127.0.0.1:9/v1` 并重启 → 发「ping」→ 恢复配置再发「恢复」。

| # | 中断侧 | 恢复后 | 判定 |
|---|---|---|---|
| 1～3 | 含连接失败/错误类提示（`has_retry_hint`） | HTTP 200 且有回复 | 通过 |

未观察到同文案重复受理（恢复轮各 1 次用户消息）。

## F3

注入：要求 coding agent；仅对**任务启动后新出现**的 `codex`/`codex-acp` 等 PID 发 `SIGKILL`（不碰本机常驻 pi）。

| # | 新 agent PID | 任务结果 | 被杀 PID 残留 | 判定 |
|---|---|---|---|---|
| 1 | 13799 | FAILED | 无 | 通过 |
| 2 | 17521 | FAILED | 无 | 通过 |
| 3 | 18413 | FAILED | 无 | 通过 |

## 偏离 / 待归属

本轮 F1～F3 **未开** `[待归属]`：行为符合 §6 期望。F4～F7 不在本卡范围。

## 复现命令（实验室）

```bash
# 专用配置 bind 18787 后：
python3 -u /tmp/line6-fault/run_f123.py
# 结果：/tmp/line6-fault/evidence/f123-results.json
```
