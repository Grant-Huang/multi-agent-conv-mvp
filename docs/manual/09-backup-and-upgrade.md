# 09 备份、升级与回滚

本页对应第 6 线可部署性检查 D5～D7（见 [LOCAL_E2E.md](../testing/LOCAL_E2E.md) §2）。**当前产品没有一键升级向导**；按下面手工步骤操作。

## 数据在哪里

| 路径（默认） | 内容 |
|---|---|
| `config.toml` | 模型、agent、开关 |
| `data/agentnexus.db`（及 `-wal` / `-shm`） | 会话、消息、任务、个人记忆等 |
| `data/agentnexus.mom.db` | MOM 沙盘库（若启用） |
| `workspace/` | General / Coding 工作目录 |
| `logs/` | JSON 行日志 |

`db_path` 可在 `[server]` 里改。

## 备份（D7）

1. **先停服务**（避免 WAL 未合并）。
2. 若存在 `*.db-wal`，用 SQLite 合并后再拷：

```bash
sqlite3 data/agentnexus.db 'PRAGMA wal_checkpoint(FULL);'
cp config.toml backup/config.toml
cp data/agentnexus.db backup/agentnexus.db
# 若有 MOM：
cp data/agentnexus.mom.db backup/agentnexus.mom.db
```

3. 恢复：停服务 → 用备份覆盖同名文件 → 用**备份时同一主版本或更新的**二进制启动。

## 升级（D5）

1. 按上一节备份。
2. 换到新版本代码 / 二进制（例如拉最新 `master` 后 `cargo build --release --bin agentnexus`）。
3. 对照新版 `config.example.toml`，把新增节（如 `[desktop]` / `[mom]` / `[introspection]`）按需写入你的 `config.toml`。
4. 用**同一 `db_path`** 启动新版本。启动时会自动补列（迁移）；不要手改库。
5. 打开桌面：旧会话、任务、记忆应仍在。

2026-11-26 实测：`v1.0.0` → 当时 `master`（`300f9b7` 线）会话与消息保留，无 panic。证据见 [local-deploy-2026-11-26.md](../testing/evidence/local-deploy-2026-11-26.md)。

## 回滚（D6）

1. 停服务，换回旧版二进制。
2. **优先用升级前的库备份**启动旧版。
3. 若直接用「已由新版写过」的库启动旧版：旧版可能仍能起来并读到旧表里的会话，但**不会提示**哪些新表/新字段（如个人记忆）在旧版不可用。不要假设新功能数据在旧版里可管理。

发现迁移失败或数据丢失时，保留日志与库副本，开 GitHub Issue（标题以 `[待归属]` 开头，标签 `dispatch` + `line:2`）。
