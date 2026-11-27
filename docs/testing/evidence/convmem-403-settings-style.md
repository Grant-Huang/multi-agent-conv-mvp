# 设置风格生效（INTENT-034 / #403，方案 a）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-02（风格生效）；决策 [2026-11-26-convmem-settings-style-effective](../../decisions/2026-11-26-convmem-settings-style-effective.md)（用户拍板方案 a） |
| 用例 | RT-USTY-07（新增） |
| 运行模式 | 无头 local；respond 假模型 + 进程内服务夹具；`cargo test -p an-server --test personal_memory` |

## 缺陷（INTENT-034）

设置面板「沟通原则（Soul）/ 质量偏好（Taste）」保存成功但回答路径从不读取——用户改了等于没改（违反 CH-G1：用户设定的东西要真实生效）。

## 拍板

用户 2026-11-26 会话选**方案 a**：设置字段作为已确认风格进风格卡。要点：保存即确认（用户主动保存 = 确认动作）；全会话生效（设置是持续配置，非一次性沉淀，不沿用记忆的会话回填限制）；**种子默认值视作「未设置」不注入**（否则每个会话凭空多出风格卡，破坏「沉淀可见可控」，也保护既有 RT-USTY 冻结断言语义）；清空字段即停用；来源 id `profile:soul` / `profile:taste`；个人质量偏好 ≠ 组织 taste（CH-G2 组织层不经个人设置）。

## Red（修复前，master `5f7d45d` + 仅新增测试）

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test personal_memory rt_usty_07
test result: FAILED. 0 passed; 1 failed; ... finished in 0.04s
```

（PATCH 设置成功后，回答提示词风格卡仍不含设置文本——缺陷本体。）

## 修复

- `store.rs`：`DEFAULT_SOUL` / `DEFAULT_TASTE` 常量化（种子与判定共用，避免双写漂移）。
- `session_materials.rs` Collector：style 段追加设置条目——字段非空且 ≠ 种子默认值时，以 `profile:soul` / `profile:taste` 为来源 id、状态「已确认（设置）」进风格卡；与 L3 风格记忆同卡渲染（护栏句、≤200 token 预算沿用 5.2b）。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test personal_memory
test result: ok. 11 passed; 0 failed; ... finished in 0.08s
```

（RT-USTY-01～06 回归同绿——默认值不注入的口径保证既有断言不受影响。）

## 边界

- 界面「改了就生效」的展示与文案归第 1 线（#306 / #347 配合）。
- 组织 taste（CH-G2 组织层）不经个人设置注入；共享层接入随 REQ-03 / 第 4 线。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
