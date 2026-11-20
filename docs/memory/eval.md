# 业务内省 × DES 植入真值评测（S7）

## 目的

用 DES 的标准答案（`GroundTruth`）核对内省是否「看到」了植入问题。数字仍只由代码统计；本说明描述如何复现评测，不是改闸门规则。

## 怎么跑

### 1. 单元 / 夹具（CI）

```bash
CARGO_TARGET_DIR=target cargo test -p an-runtime --test introspection_explain
```

- `rt_int_15_*`：模型解释开启但无槽位 → `introspection.job_skipped`（原因含「无槽位」），确定性 Finding 仍在、无解释文字。
- `des_eval_covers_planted_alarm_assets_in_evidence`：`GroundTruth.alarms[].asset` 出现在 Finding 证据 `case_id` / 文本中。

API：`an_runtime::introspect::{coverage_against_truth, assert_adoption_stats, truth_alarm_assets}`。

### 2. 手工对照 DES 真值

1. 用已知 `seed` / `pattern` 跑 `an_des::run_open_loop` 或 MOM `create_des_simulation`，取出 `des_review_truth`（只读）。
2. 将运营案例摘要映射为 `MemorySource` / `MomSource`（案例 id 中带上设备号便于覆盖检查）。
3. `analyze_adoption` → `coverage_against_truth(&truth, &drafts)`。
4. 期望：`assets_missing` 为空，或文档说明为何该植入未进入采纳主题（例如无审批样本）。

## 配置

```toml
[introspection]
enabled = false          # 默认关
model_explain = false    # S7 模型解释默认关；开时抢后台槽位，等不到就跳过
```

## 约束

- 模型解释**不得**改 `AdoptionStats` 字段。
- 不抢前台槽位；`acquire(..., max_wait=0)`。
- 真值不得进入 agent/human 观测视图（既有 RT-DES-04）。
