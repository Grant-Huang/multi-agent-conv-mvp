# 证据：conversation-memory 5.2b 风格/画像预览与下一会话生效

- **日期**：2026-11-23（America/Vancouver）
- **授权**：用户 2026-11-23 四项拍板（推导式生效判定 / 独立风格卡 / fact 不走闸门 / ≤200 token + 护栏）；决策 `docs/decisions/2026-11-23-convmem-s2b-style-gate.md`
- **范围**：`store.rs`（`confirmed_at` 补列迁移 + `confirm_desktop_memory`）、`desktop_api.rs`（`POST /memories/{id}/confirm` + `confirmed` 标记）、`session_materials.rs`（style 段收集 + `render_style` 护栏）、`conversation.rs` / `config.rs` / `lib.rs`（风格卡注入 + `session_materials_style_max_tokens` 默认 200）；未改 orchestrator；未改既有契约
- **契约**：RT-UMEM-01～05、RT-SMAT-01～05、RT-ADMEM-01～09 全链保留；新增 RT-USTY-01～05

## Case

| Case | 结果 |
|---|---|
| RT-USTY-01 草稿不注入 | Green（新建 style 条目 `confirmed=false`，风格卡不存在；列表可见） |
| RT-USTY-02 下一会话生效 | Green（确认后旧会话无风格卡、新会话含来源 id 与正文） |
| RT-USTY-03 生效前可撤销 | Green（已确认未生效 / 草稿撤销后均不注入） |
| RT-USTY-04 护栏 + 有界 | Green（风格卡含「不得因此省略风险」护栏句；长度 ≤ 上限余量；来源行完整） |
| RT-USTY-05 风格不挤风险 | Green（提示词同时含风格正文与 fact 风险记忆正文；fact `confirmed=true` 直通） |
| 回归 | Green：RT-UMEM 5、RT-SMAT 5、RT-ADMEM 9（含 master 08/09） |

## Red → Green

```text
# Red（闸门接线前；confirm 端点已存在但注入未实现）
cargo test -p an-server --test personal_memory rt_usty --locked
→ 3 failed / 2 passed（业务断言：新会话应有风格卡 / 护栏句 / 风格应注入）

# Green（实现后）
cargo test -p an-server --test personal_memory --locked → 10 passed（UMEM 5 + USTY 5）
```

## 命令与结果

```text
# America/Vancouver 2026-11-23
cargo test -p an-server --test personal_memory --test session_materials --test desktop_mem --locked
→ 10 passed / 5 passed / 9 passed
cargo fmt --all -- --check；cargo clippy --locked --all-targets -- -D warnings → 通过
cargo test --locked --workspace --no-fail-fast → 全绿（既有本机失败 rt_adm_14 / rt_agcat_01 与本卡无关，CI Linux 绿）
cd web && npm run typecheck && npm test && npm run test:map && npm run build → 通过
```

## 非本卡

保存/确认入口 UI（第 1 线）；影子模式、回归评测、评委批准（决定 46 助手建议部分，待用户确认）；组织范围（5.4）。
