# routing-and-admission：前台提速实验与 agent 准入（均无界面入口）

## A. front-thinking：前台判断提速实验

只通过 `config.toml` 打开，默认全关；不写新配置时请求与原来完全一致。

### Sub-features

| 开关 | 位置 | 行为 |
|---|---|---|
| `front_structured_thinking` | `[backends.<名>]` | 关闭 conversation/router 结构化请求的思考 |
| `front_reason_first` | 同上 | 不思考时，reason 字段排在最前（先写 ≤40 字依据） |
| `front_thinking_escalation` | 同上 | 格式不合格、风险 high、角色无可用 agent、任务目标为空时，开思考重判一次，不退回不思考 |
| `risk_rules` | `[conversation]` | 规则判风险，只升不降（删除/对外发送/部署 → high；写文件/改代码 → 至少 medium） |
| `route_class_questions` | `[conversation]` | 路由回答两个是/否问题，类别由代码算出（本机实验未通过，见 DECISIONS 第 42 条） |
| `route_class_from_tools` | `[conversation]` | general 任务类别由工具清单算出 |

### How to get to it / Drive

改配置后重启；用 `an-eval --variant E4` 临时套用变体对比，不必改文件：

```bash
cargo run --release --bin an-eval -- config.toml evals/routing.jsonl --variant E4 --label E4
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test front_thinking   # RT-FT-01～20，假模型服务记录请求体
```

### Verify / Gotchas

- 断言点是**发给模型的请求体**，不是页面表现；真实模型的准确率与延迟必须在本机评测（FT.3、FT.5）。
- RT-FT 中部分阶段是基线守护（用例 01/03/05/11/12/17/19/20），其余原为 Red→Green，别把基线当新功能证据。

## B. admission：外部 agent 准入与持续信任（E3，`[admission] enabled = true` 才有）

### Sub-features

- 准入流程 S1→S7：澄清、加权打分与等级（Guest/Member/Trusted）、人工审批闸门、确定性抽检、警告与降级、撤权与凭证代际、重启恢复
- 按等级分配任务（Guest 仅低风险、Member 到中风险、Trusted 不限）
- 凭证过期的结果被丢弃；撤权中止进行中任务并由备份接手
- 外部 agent（`kind = "a2a"`）经 HTTP 适配器激活后注册并可完成任务；重启后自动重新注册
- 已 Active 的 agent 不可就地改写接入地址/角色/类型/并发（重复 activate 返回 409，记录与审计事件不变）

### How to get to it

HTTP `/api/admission/*`（未开启时 404）；实现 `crates/server/src/admission_api.rs`。**未接入网页界面。**

### Drive

```bash
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test admission   # RT-ADM（runtime）
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test admission    # HTTP
cargo run -p an-runtime --example admission_replay        # 用正式账本回放 A1/A2/A3
cargo run -p an-server --example admission_revoke_bench   # 撤权实测
```

### Verify / Gotchas

- 判据来自账本与事件：等级、凭证代际、审计事件与注册卡片一致。
- **没有身份认证、没有 endpoint 网络白名单**；S2/S3 的大模型澄清与评分由实验脚本完成，不是线上闭环。
- 准入的审批与普通任务审批（[approval.md](approval.md)）是两套机制。

## Backed by

源码：`crates/runtime/src/admission.rs`、`policy.rs`、`crates/server/src/admission_api.rs`
用例：RT-FT-01～20、RT-ADM-01～18、`server/tests/admission.rs`
验证状态：运行时自动化覆盖；浏览器无。
