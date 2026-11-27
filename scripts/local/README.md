# 本地长程 / 故障辅助脚本（第 6 线）

不改 `crates/`、`web/src/`。权威设计见 `docs/testing/LOCAL_E2E.md` §5（W4）与 §6（W5）。

## soak.py

按“工作周”时间表驱动已启动的 AgentNexus：

```bash
python3 scripts/local/soak.py \
  --base http://127.0.0.1:8787 \
  --duration-hours 8 \
  --day-minutes 240 \
  --min-interval 120 --max-interval 300 \
  --out /tmp/line6-soak/events.jsonl
```

- `--day-minutes`：压缩一个“工作日”的墙钟分钟数（含上班+离开）。
- `--work-ratio`：日长中上班占比（默认 0.75）。

## soak_metrics.py

每 N 秒采样后端 RSS、FD、监听端口、DB/日志体积、agent 子进程数，写 CSV：

```bash
python3 scripts/local/soak_metrics.py \
  --pid "$(lsof -nP -iTCP:8787 -sTCP:LISTEN -t | head -1)" \
  --db data/agentnexus.db \
  --log-dir logs \
  --out /tmp/line6-soak/metrics.csv \
  --interval 300 \
  --duration-hours 8
```

## 阶梯

1. 脚本冒烟（压缩时钟，≤1h）
2. **8h 冒烟**（真实模型 + ≥1 本机 agent）
3. 24h → 72h

证据写 `docs/testing/evidence/local-soak-<日期>.md`。
