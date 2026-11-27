# 连接器注册表 connectors.toml + mom_relos 迁移（#487，契约 K5/K6 + §7 拍板）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 契约 | [connector.md](../../contracts/connector.md)（#477 批准；§7 三项拍板见[决策](../../decisions/2026-11-27-connector-k7-decisions.md)）；迁移卡 #487 |
| 用例 | connectors 注册表单测 ×3（crates/runtime --lib）；bearer header 测试（crates/server --lib） |
| 运行模式 | `cargo test -p an-runtime --lib connectors` / `-p an-server --lib` |

## 实现

- **`crates/runtime/src/connectors.rs`**：`connectors.toml` 解析（`[[connector]]` 描述符：id/kind/transport/endpoint/credentials.env 引用/capabilities/health/owner）→ `Registry::lookup(id)`；文件缺失 = 空注册表（Ok，迁移期不制造告警噪音），解析失败 = Err（配置错误显式暴露）。
- **an-mom 装配**（拍板②先告警后拒绝）：注册表 `id="relos"` 命中 → 用 `endpoint` + K2 凭据引用（env 解析，缺失 WARN `connector.credential_missing`）构造 RelosClient；未命中 → WARN `connector.unregistered` + 回退旧 `--relos-url` 配置。路径默认 `connectors.toml`，env `CONNECTORS_TOML` 可覆盖。
- **mom_relos.rs**：`RelosClient::with_bearer(base_url, credential)`——凭据设置则请求带 `Authorization: Bearer`；未启用鉴权的服务传 None 不受影响。
- **样例**：`connectors.example.toml`（relos 条目示例）；`config.example.toml` 注释 `connectors_path`。

## 单元测试

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --lib connectors
test result: ok. 3 passed; 0 failed; ... （样例解析+查找 / 缺失文件=空表 / 坏 toml=Err）
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --lib
test mom_relos::bearer_tests::bearer_header_follows_credential ... ok
test result: ok. 6 passed; 0 failed
```

（bearer 测试：带凭据请求带 `Authorization: Bearer test-bearer-487`，无凭据请求无该头。）

## 迁移语义（拍板②）

- 注册表命中 → 走注册表 endpoint（+凭据）。
- 未命中 → WARN `connector.unregistered` + 回退旧 `--relos-url`；迁移期满翻转「未注册 → 拒绝」（时点随里程碑由用户拍板）。
- 既有 mom.rs E1-C 测试（server_with_relos 直连 stub）全部不受影响——回退路径保持。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
