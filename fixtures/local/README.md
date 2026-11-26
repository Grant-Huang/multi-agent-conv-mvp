# 本地演示厂（local-demo-plant）

第 6 线 W2 数据集定义。版本见 `VERSION`。

```bash
python3 scripts/local/seed.py create --base http://127.0.0.1:8787 --scale small
python3 scripts/local/seed.py verify --base http://127.0.0.1:8787 --scale small
```

- `--skip-live`：不发 desktop requests（不打模型），只建记忆与会话壳。
- `--live-turns N`：每用户 N 条 live 请求（含注入请求文案）。
- 需要：`[mom] world_mock=true`、建议 `[desktop] test_fixtures=true`、`[introspection] enabled`（内省）。
