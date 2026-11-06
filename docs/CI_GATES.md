# CI 门禁设计

本文说明仓库的 CI 门禁：每一道门禁检查什么、阈值从哪来、如何在 GitHub 上设为必需检查，
以及目前**没有**覆盖的部分。

- 工作流：`.github/workflows/ci.yml`
- 风险分级策略：`config/risk-policy.toml`
- 风险分级脚本：`tools/ci/risk_classify.py`
- 变更规模脚本：`tools/ci/change_size_check.py`
- 代码负责人：`.github/CODEOWNERS`
- 上位设计：`docs/AUTONOMOUS_DEV_ACTION_GUIDE.md`（第 1~3 层，附录 A.6、A.9、A.12）

> 状态：本文和脚本已在本地验证（见下文"本地验证"）。**GitHub Actions 工作流尚未在真实 PR 上运行过**，
> 首次运行前需要人工确认各 job 通过。

## 1. 门禁一览

| job（必需检查名） | 检查内容 | 失败条件 | 对应 AGENTS.md 完成定义 |
|---|---|---|---|
| `rust` | `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test --workspace`、ts-rs 绑定是否最新 | 任一命令非零退出，或绑定有未提交差异 | `cargo test`、`cargo clippy` 无警告 |
| `web` | `npm run typecheck`、`npm test`、`npm run build` | 任一步骤失败 | `npm run build` |
| `python-websearch` | `tools/websearch` 的 pytest（覆盖率 ≥ 80%）与 flake8 | 测试失败、覆盖率不足、lint 报错 | — |
| `python-tools` | `tools/ci` 策略脚本的测试与 flake8 | 同上 | — |
| `change-policy` | 风险分级（只输出结论）+ 变更规模（可阻断） | 变更规模阻断（见第 2 节） | — |
| `security` | `cargo audit`、gitleaks 全历史扫描 | 发现已知漏洞或密钥 | — |

`change-policy` 只在 PR 上运行。风险等级本身不会让 job 失败：等级决定的是**审批要求**，
而审批要求只能由 GitHub 的分支保护和 CODEOWNERS 强制执行（见第 3 节）。

## 2. 风险分级与变更规模

### 2.1 风险等级（L0~L3）

等级由 `config/risk-policy.toml` 的路径规则计算，规则如下：

- 每个改动文件取所有匹配规则中**最高**的等级；整个 PR 取所有文件中的最高等级。
- **未匹配任何规则的文件使用默认等级 L2**，不会被忽略。新增目录忘记登记时，宁可多审也不漏审。
- 内容信号只看**新增行**，命中后把该文件至少提到信号等级；信号只能升级，不能降级。
- 模型或作者的自述不参与计算。

| 等级 | 典型路径 | 审批要求（人工强制） |
|---|---|---|
| L0 | `docs/**`、`evals/**`、`*.md` | 自动合并允许（仍需 CI 通过） |
| L1 | `web/**`、路由 / 组装 / 提示词 | 1 位审查人批准 |
| L2 | 状态机、持久化、子进程、公共接口、`Cargo.toml` | 1 位审查人批准，且不是作者本人 |
| L3 | `.github/**`、`config/**`、`tools/ci/**`、`AGENTS.md`、权限策略、节点与远程 agent | 模块负责人（CODEOWNERS）批准 |

### 2.2 变更规模

统计口径：新增行 + 删除行；**排除**生成文件与锁文件（`web/src/bindings/*`、`Cargo.lock`、
`web/package-lock.json`、`tools/websearch/uv.lock`、`evals/results/*`）。

| 规则 | 阈值 | 结果 |
|---|---|---|
| 行数或文件数 | ≥ 800 行 或 ≥ 20 个文件 | 警告 |
| 行数或文件数 | ≥ 2000 行 或 ≥ 40 个文件 | **阻断**；PR 描述含 `## 大改动说明`（≥ 40 字）则降为警告 |
| 组件数（不计 docs、evals） | > 3 | 警告，建议拆分 |
| 删除的测试标记多于新增 | — | **阻断**；PR 描述含 `## 测试删除说明`（≥ 20 字）则降为警告 |

阈值依据：本仓库最近 25 个非合并提交多为 500~1500 行，少数超过 3000 行。
800 / 2000 是**待确认的建议值**，见第 5 节。

### 2.3 本地验证

```bash
python3 tools/ci/risk_classify.py --base origin/master
PR_BODY="" python3 tools/ci/change_size_check.py --base origin/master
cd tools/ci && uv run --no-project --python 3.13 --with pytest python -m pytest -q
```

脚本只用标准库（Python ≥ 3.11），测试额外需要 pytest。

## 3. 在 GitHub 上设置分支保护

在仓库 Settings → Branches → `master` 的保护规则中：

1. 勾选 **Require a pull request before merging**，并设置 **Require approvals**（至少 1）。
2. 勾选 **Require review from Code Owners**（让 CODEOWNERS 生效）。
3. 勾选 **Dismiss stale pull request approvals when new commits are pushed**。
4. 勾选 **Require status checks to pass before merging**，并把下列检查设为必需：
   `rust`、`web`、`python-websearch`、`python-tools`、`change-policy`、`security`。
5. 勾选 **Do not allow bypassing the above settings**，并禁止直接推送 `master`。

### 3.1 GitHub 无法表达的规则

- **"L3 需要两名审查人"** 无法用分支保护直接表达。当前只能靠 CODEOWNERS 要求负责人批准。
  如需双人审批，应改用 GitHub 的 Rulesets 或在 `change-policy` 中检查审查人数量（尚未实现）。
- **"审查人不能是作者"** 同样需要 Rulesets 或自定义检查；目前依靠人工遵守。
- **风险等级本身不会阻止合并**：`change-policy` 只输出等级，不会因为 L3 而失败。

## 4. 目前没有覆盖的部分

- 没有 pre-commit 钩子（本地不会自动格式化或检查）。
- 没有端到端测试在 CI 中运行（`cargo test --workspace` 包含 `tests/e2e.rs`，已覆盖）。
- Python 测试标记为 `live` 的用例访问真实网络，CI 默认跳过。
- 测试删除检测只按标记行计数（`#[test]`、`def test_`、`it(` 等），**不能证明测试被弱化**，只提醒人工确认。
- 风险分级只看路径和新增行，无法理解语义。涉及行为变化但路径未登记的改动，依赖默认等级 L2 兜底。

## 5. 待办与待人工确认

- [ ] **确认 CODEOWNERS 的负责人**：当前全部为 `@Grant-Huang`（仓库所有者）。
- [ ] **确认变更规模阈值**（800 / 2000 行，20 / 40 文件）。
- [ ] **固定第三方 action 版本**：工作流目前用主版本标签（`@v4` 等）。正式启用前应替换为具体 commit SHA，
      并由维护者核对，本次未填写任何 SHA。
- [ ] **首次 CI 运行后核对**：Rust 工具链、系统依赖、gitleaks 与 cargo-audit 的结果。
- [ ] 一次性 `cargo fmt --all` 已作为 PR 的单独提交加入；之后 `cargo fmt --check` 才能通过。
