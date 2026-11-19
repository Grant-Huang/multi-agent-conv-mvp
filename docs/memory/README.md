# 记忆、画像与业务内省（docs/memory）

本目录只服务两件事：**业务内省（INT，已开干）** 与 **个人风格 / 画像 / taste（尚未开干）**。

| 文件 | 用途 | 谁读 |
|---|---|---|
| [INTROSPECTION_PLAN.md](INTROSPECTION_PLAN.md) | 业务内省总方案（已批准）；模块、表、闸门、接口 | 改设计前必读 |
| [INTROSPECTION_PLAYBOOK.md](INTROSPECTION_PLAYBOOK.md) | **可执行说明书**：S2–S8 逐步怎么写测试、怎么实现、验收出口 | **接手实现的 agent 从这里开始** |
| [ACCEPTANCE.md](ACCEPTANCE.md) | 验收用例；业务内省部分已冻结 | 写测试时对照断言，**不改断言** |
| `docs/DECISIONS.md` 第 45–55 条 | 已拍板的决策 | 冲突时以决策为准 |
| `docs/PLAN.md`「业务内省（INT）」一节 | 进度勾选 | 每步完成后更新 |

## 进度（业务内省）

| 步 | 状态 | 说明 |
|---|---|---|
| S0 | ✅ | 用例冻结、方案批准（PR #107） |
| S1 | ✅（PR #122） | 5 张新表 + `Store` 读写；11 个 `int_store_*` 单元测试 |
| S2 | ✅ | SourceAdapter / 采纳模式 / 校验器 |
| S3–S8 | ⏳ | 见 [INTROSPECTION_PLAYBOOK.md](INTROSPECTION_PLAYBOOK.md) |

## 接手规则（弱 agent 也必须遵守）

1. **先读** `INTROSPECTION_PLAYBOOK.md` 当前步骤整节，再动代码。
2. **先 Red 后 Green**：业务断言失败才算 Red；环境失败（代理、端口、沙箱）不算。
3. **不改冻结断言**：`ACCEPTANCE.md` 里业务内省部分、已并入 `CASES.md` 的行、已写好的测试断言，一律不改；怀疑有误先停下来问人。
4. **一步一个 PR**；默认 `[introspection] enabled = false`；只新建表、不改已有表；不碰 `orchestrator.rs` / `registry.rs` / `acp.rs` / `util.rs` / MOM 写接口。
5. 声称完成前：`cargo fmt`、`CARGO_TARGET_DIR=target cargo test`、`cargo clippy --all-targets`、`cd web && npm run build`、`npm run test:map`，并把实际输出贴进回复。
6. 决策编号容易撞号：**新增决策前先看 `origin/master` 上 `docs/DECISIONS.md` 最大编号**。
