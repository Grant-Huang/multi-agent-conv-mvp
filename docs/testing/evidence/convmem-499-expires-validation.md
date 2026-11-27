# 记忆有效期校验 + 存量非 ASCII 不 panic（R-20261127-H3 / #499）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-27 |
| 特性 | conversation-memory；REQ-conversation-memory-02 用例族 |
| 用例 | RT-UMEM-08（写入端校验）、RT-UMEM-09（存量不 panic）；memory_expired 单测（crates/runtime --lib） |
| 来源 | 夜间评审 2026-11-27（PR #497）台账 id=R-20261127-H3（已复现） |
| 运行模式 | 无头 local；文件库 + rusqlite 回填模拟存量；`cargo test -p an-server --test personal_memory` |

## 缺陷（已复现）

`session_materials.rs` `memory_expired` 用 `&e[..10]` 字节切片：用户在设置面板把有效期改成「长期有效」→ 每轮对话 panic（`end byte index 10 is not a char boundary`）→ 500 回合异常；「2026/12/01」被当成永不过期。违反 REVIEW 3.3 第 4 条（生产路径 panic）。

## 实现

- `memory_expired` 改 `e.get(..10)`——非 ASCII / 超长 / 斜杠日期一律「未过期」，不 panic；并要求整串恰为 10 字符（`2026-12-011` 不再被前缀误判）。单测 5 态（长期有效 / 斜杠 / 中文自由文本 / 过去日期 / 未来日期 / 长期）。
- **写入端校验**（`store.rs validate_expires`，create 与 patch 双口径 + patch handler 预检 400）：只允许「长期」或 `YYYY-MM-DD`；非法值 400 且库里不变。存量数据不受影响（读取侧兼容）。

## Red（修复前，master `b113aeb` + 仅新增测试）

```
rt_umem_08_expires_validation_rejects_invalid_formats ... FAILED（非法值 201≠400）
rt_umem_09_legacy_non_ascii_expires_no_panic ... FAILED（对话 panic）
test result: FAILED. 13 passed; 2 failed
```

rt_umem_09 的 panic 复现即 issue 的「已复现」本体。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test personal_memory
test result: ok. 15 passed; 0 failed; ... finished in 0.09s
$ CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --lib memory_expired
test result: ok. 1 passed; ... （非 ASCII / 斜杠 / 中文自由文本不 panic 五态）
```

## 边界

- 存量自由文本（校验上线前入库）读取侧兼容为「未过期」；要改为真实过期需重新编辑为合法格式（界面配合 #347）。

## 门禁

fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
