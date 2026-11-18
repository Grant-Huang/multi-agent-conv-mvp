# FT.1 / FT.2 前台提速证据

日期：2026-11-17。用户确认：以 D（先快后慢）为主，配合 A（先写理由）、B（规则判风险）；验收准确率优先（P95 放宽到 6 秒）；新增 3 个配置项，默认全关。

## Red → Green

`CARGO_TARGET_DIR=target cargo test --locked -p an-server --test front_thinking`

- FT.1（实现前）：`4 passed; 7 failed`。失败均为业务断言：RT-FT-02 第一个字段仍是 `action`；RT-FT-04 "删除生产环境数据库里的旧表" 风险仍为 `Low`；RT-FT-06 / 09 / 10 重试仍不思考；RT-FT-07 风险仍为 `High`；RT-FT-08 直接报错"重判后应路由成功"。01 / 03 / 05 / 11 为默认行为不变的守护用例。
- FT.2（实现后，断言未改）：`11 passed; 0 failed`。
- 另加 `an-eval` 单元测试 `ft_variants_apply_to_all_backends`。

## 改动与边界

- 新配置默认全关；A、D 只在 `front_structured_thinking = false` 时生效（`LlmClient::with_front_options` 在开着思考时忽略它们）。不写新配置时请求体与以前完全相同（RT-FT-01）。
- D：开了升级时，不思考的第一次只试一次（`with_max_retries(0)`）；格式 / 业务校验不合格、风险 high、角色没有可用 agent 时，用 `with_thinking()` 重判一次，重判按原有 `max_retries` 重试，不会再退回不思考（RT-FT-10）。传输错误不触发重判。
- B：关键词只升不降，放在 router 后处理，不改 `policy.rs`。去掉了会误命中 `prefix` 等词的 `fix`。
- 超时与取消：沿用现有 `chat_json` 的超时、期限和取消；重判是同一个 future 里的第二次调用，调用方取消时一起取消。重判会让该轮多一次完整的思考调用，这是 D 的设计代价，由 FT.3 实测衡量。
- 新事件：`llm.escalated`、`route.risk_raised`（DESIGN.md 10.3）；新配置写入 DESIGN.md 第 12 节和 `config.example.toml`。

未验证：llama.cpp 是否按 schema 属性顺序生成 JSON（A 依赖这一点，FT.3 用原始输出确认）；真实 35B 的准确率和延迟（FT.3 在用户本机跑）。

## 参考值（强模型盲答）

Claude 在会话中盲答当前 100 条评测集（审查模式 reviewer），按 `an-eval` 的检查规则打分：100/100。只作上限参照，限制见 docs/PLAN.md 的 FT 一节。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked：432 passed; 0 failed; 2 ignored（原有）；exit=0
cargo clippy --locked --all-targets -- -D warnings：exit=0
web npm test：Tests 21 passed (21)
web npm run build：✓ built in 1.27s
web npm run test:map：Feature map valid: 20 features, 14 browser cases
```

本地结果不代替 GitHub CI。
