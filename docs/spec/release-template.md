# 版本说明模板

每次发版写一份 `docs/releases/vX.Y.Z.md`，与 git tag `vX.Y.Z` 对应。第 2、3 节由登记表 `docs/testing/features.json` 生成（生成脚本在首次按新规范发版时补），其余人工填写。规则见 [DEV_STANDARD.md](../../DEV_STANDARD.md) 第 9 节；里程碑准出时同时填写第 7 节结论。下面代码块里是模板正文。

```markdown
# vX.Y.Z

| 项目 | 内容 |
|---|---|
| 日期 | YYYY-MM-DD |
| tag 指向 | <提交号> |
| 上一版本 | vX.Y.Z |
| 里程碑 | 例如 World Adapter M1 准出 |

## 1. 一句话
这个版本对用户意味着什么。

## 2. 包含的需求
| 特性 | 需求 | 状态 | 用例 | 证据 |
|---|---|---|---|---|
| world-adapter | REQ-world-adapter-01 | 已发布 | RT-WADES-01 | docs/testing/evidence/… |

## 3. 质量
- CI：<运行链接或编号>，结果
- 浏览器 E2E / 合同测试：通过数 / 失败数 / 跳过数
- 仍用 Mock 的部分：见各特性档案

## 4. 已知问题
- 台账编号、影响、规避方式

## 5. 升级与回滚
- 配置变化、数据迁移、回滚方式

## 6. 相关决策
- decisions/…

## 7. 里程碑准出（里程碑版本必填）
| 方面 | 结论 | 证据 |
|---|---|---|
| 业务验收 | PASS / FAIL / NOT_VERIFIED | …… |
| 架构与契约 | PASS / FAIL / NOT_VERIFIED | …… |
| 安全与权限 | PASS / FAIL / NOT_VERIFIED | …… |
| CI、回归、端到端 | PASS / FAIL / NOT_VERIFIED | …… |
| 部署与可观测 | PASS / FAIL / NOT_VERIFIED | …… |
| 恢复演练 | PASS / FAIL / NOT_VERIFIED | …… |

- 未关闭的关键问题：……
- 非关键剩余风险（负责人、缓解措施、到期日）：……
- **结论**：PASS / CONDITIONAL / BLOCKED（CONDITIONAL 不适用于安全或不可逆问题）
- 批准人 / 时间 / 证据链接：……（必须真实）
```
