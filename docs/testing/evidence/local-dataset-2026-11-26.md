# 本地演示厂数据集证据（W2 / #425）

- **日期**：2026-11-26（America/Vancouver）
- **测口**：`http://127.0.0.1:8787`
- **版本**：`local-demo-plant` 0.1.0 / scale=`small` / seed=`42425001`
- **命令**：

```bash
python3 scripts/local/seed.py create --base http://127.0.0.1:8787 --scale small --skip-live
python3 scripts/local/seed.py verify --base http://127.0.0.1:8787 --scale small
```

## create 结果（摘录）

- `world/mock/ops/ensure-seed` → HTTP 200（`seeded` / `case_id`）
- `snapshot/fixture/reset` → HTTP 200；`tasks_visible=37`
- 6 用户 × 记忆约 11 条写入路径；历史会话壳每人 30 → 合计 180
- `introspection/run` → HTTP 200
- 另：`qa-line3` 一条 live `POST .../requests` → HTTP 202 `accepted`

## verify

`VERIFY PASS`（history-like convs≥180；每用户 memories≥5 且含注入标题；industrial ensure-seed 200；tasks≥1）。

## 已知缺口

公开 API **无法**在不调用 LLM 的情况下批量写入历史消息正文；small 用会话壳 + 可选 live 轮次。已开 `[待归属]` Issue 给第 2 线。

## 运行注意

本机若 ACP 探测误杀进程组，请用**新会话**启动后端（例如 Python `start_new_session=True`），并建议 `[acp] auto_detect=false`。工业种子需 `[mom] world_mock=true`。
