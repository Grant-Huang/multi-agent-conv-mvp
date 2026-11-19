# CI 门禁设计

本文说明仓库的 CI 门禁：每一道门禁检查什么、阈值从哪来、如何在 GitHub 上设为必需检查，
以及目前**没有**覆盖的部分。

- 工作流：`.github/workflows/ci.yml`（另有影子审查、夜间 E2E）
- 风险分级策略：`config/risk-policy.toml`
- 风险分级脚本：`tools/ci/risk_classify.py`
- 变更规模脚本：`tools/ci/change_size_check.py`
- 提示注入扫描脚本：`tools/ci/injection_scan.py`
- PR 模板：`.github/pull_request_template.md`；依赖更新：`.github/dependabot.yml`
- 定期检查手册：`7-fence-review.md`（含围栏差距清单与提示注入专项检查）
- 代码负责人：`.github/CODEOWNERS`
- 围栏落地：`docs/fencing/`（分支保护、bot 身份、影子审查、Eval、产品映射）
- 上位设计：`docs/AUTONOMOUS_DEV_ACTION_GUIDE.md`（第 1~3 层，附录 A.6、A.9、A.12）
- 工具链：`rust-toolchain.toml`（固定 1.100.0）

> 状态（2026-11-19 围栏落地）：
> - CI 六类 job + `python-dlp-egress` 已在真实 PR 上跑过。
> - Action 已固定到 commit SHA；`npm run test:map` 已进 `web` job。
> - 重命名绕过已修（`--no-renames`）。
> - **分支保护须仓库所有者运行** `./scripts/enable_branch_protection.sh`（Agent 无 admin）。
> - **L0 自动合并默认不开**；阶段 2 bot 身份就绪前批准数保持 0。

## 1. 门禁一览

| job（必需检查名） | 检查内容 | 失败条件 | 对应 AGENTS.md 完成定义 |
|---|---|---|---|
| `rust` | `cargo fmt --check`、`cargo clippy --locked -D warnings`、`cargo test --locked --workspace`、ts-rs 绑定是否最新 | 任一命令非零退出，或绑定有未提交差异 | `cargo test`、`cargo clippy` 无警告 |
| `web` | `npm run typecheck`、`npm test`、`npm run test:map`、`npm run build` | 任一步骤失败 | `npm run build` |
| `python-websearch` | `tools/websearch` 的 pytest（覆盖率 ≥ 80%）与 flake8 | 测试失败、覆盖率不足、lint 报错 | — |
| `python-tools` | `tools/ci` 策略脚本的测试与 flake8 | 同上 | — |
| `change-policy` | 风险分级（只输出结论）+ 变更规模 / 测试弱化（可阻断）+ 提示注入扫描（可阻断） | 变更规模或测试弱化阻断；注入扫描阻断 | — |
| `security` | `cargo audit`、`npm audit --omit=dev --audit-level=high`、gitleaks 全历史扫描 | 发现已知漏洞或密钥 | — |

建议一并关注（可设为必需）：`python-dlp-egress`。

非必需：`pr-shadow-review`（影子评论）、`nightly-e2e`（定时）。

`change-policy` 只在 PR 上运行。风险等级本身不会让 job 失败：等级决定的是**审批要求**，
而审批要求只能由 GitHub 的分支保护和 CODEOWNERS 强制执行（见第 3 节）。

## 2. 风险分级与变更规模

### 2.1 风险等级（L0~L3）

等级由 `config/risk-policy.toml` 的路径规则计算，规则如下：

- 每个改动文件取所有匹配规则中**最高**的等级；整个 PR 取所有文件中的最高等级。
- **未匹配任何规则的文件使用默认等级 L2**，不会被忽略。新增目录忘记登记时，宁可多审也不漏审。
- 内容信号只看**新增行**，命中后把该文件至少提到信号等级；信号只能升级，不能降级。
- 模型或作者的自述不参与计算。
- git 差异使用 `--no-renames`：重命名按删旧 + 增新计，两边路径都分级，避免迁入 `.github` 被判成 L0。

| 等级 | 典型路径 | 审批要求（人工强制） |
|---|---|---|
| L0 | `docs/**`、`evals/**`、`*.md` | 允许自动合并的**策略文案**；**当前未开启**自动合并 |
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
| 测试弱化（新增 ignore/skip/only/should_panic，或 assert 净减少） | — | **阻断**；`## 测试变更说明`（≥ 20 字）则降为警告 |
| 非文档改动缺 `## 非目标` 或 Case/Feature ID | — | 警告 |

阈值依据：本仓库最近 25 个非合并提交多为 500~1500 行，少数超过 3000 行。
800 / 2000 是**待确认的建议值**，见第 5 节。

### 2.4 提示注入扫描

只看新增行，规则细节和局限见 `tools/ci/injection_scan.py` 头部注释与 `7-fence-review.md` 第 4 节。

| 规则 | 结果 |
|---|---|
| 不可见字符（零宽、双向控制、Unicode 标签） | **阻断**，不可降级 |
| 工作流里外部可控内容（PR 标题 / 正文 / 分支名 / 评论等）直接出现在 `run:` 等非 `env` 位置 | **阻断**，不可降级 |
| 注入短语出现在代码、配置、评测数据、AGENTS.md / CLAUDE.md | **阻断**；PR 描述含 `## 注入扫描说明`（≥ 20 字）则降为警告 |
| 注入短语出现在普通文档；Markdown 里的长 HTML 注释 | 警告 |

### 2.3 本地验证

```bash
python3 tools/ci/risk_classify.py --base origin/master
PR_BODY="" python3 tools/ci/change_size_check.py --base origin/master
PR_BODY="" python3 tools/ci/injection_scan.py --base origin/master
cd tools/ci && uv run --no-project --python 3.13 --with pytest python -m pytest -q
```

脚本只用标准库（Python ≥ 3.11），测试额外需要 pytest。

## 3. 在 GitHub 上设置分支保护

优先使用脚本（所有者本机、需 admin）：

```bash
./scripts/enable_branch_protection.sh
```

或在仓库 Settings → Branches → `master` 的保护规则中：

1. 勾选 **Require a pull request before merging**；阶段 0 批准数 **0**，阶段 2 起至少 **1**。
2. 阶段 2：勾选 **Require review from Code Owners**。
3. 勾选 **Dismiss stale pull request approvals when new commits are pushed**。
4. 勾选 **Require status checks to pass before merging**，并把下列检查设为必需：
   `rust`、`web`、`python-websearch`、`python-tools`、`change-policy`、`security`。
5. 勾选 **Do not allow bypassing the above settings**，并禁止直接推送 `master`。

细节见 `docs/fencing/BRANCH_PROTECTION.md`。

### 3.1 GitHub 无法表达的规则

- **"L3 需要两名审查人"** 无法用分支保护直接表达。当前只能靠 CODEOWNERS 要求负责人批准。
  如需双人审批，应改用 GitHub 的 Rulesets 或在 `change-policy` 中检查审查人数量（尚未实现）。
- **"审查人不能是作者"**：阶段 2 用 bot 提 PR + 人批准解决结构问题。
- **风险等级本身不会阻止合并**：`change-policy` 只输出等级，不会因为 L3 而失败。

## 4. 目前没有覆盖的部分

- 没有 pre-commit 钩子（本地不会自动格式化或检查）。
- 浏览器 E2E 在夜间 job，不在每次 PR（`cargo test --workspace` 仍含后端 e2e）。
- Python 测试标记为 `live` 的用例访问真实网络，CI 默认跳过。
- 测试弱化检测是启发式，不能证明语义上「断言变松」的全部情况。
- 风险分级只看路径和新增行，无法理解语义。涉及行为变化但路径未登记的改动，依赖默认等级 L2 兜底。
- 影子审查不阻断；独立审批依赖所有者配置 bot 与分支保护。

## 5. 待办与待人工确认

- [ ] **所有者执行** `./scripts/enable_branch_protection.sh`（阶段 0）
- [ ] **确认 CODEOWNERS 的负责人**：当前全部为 `@Grant-Huang`（仓库所有者）。
- [ ] **确认变更规模阈值**（800 / 2000 行，20 / 40 文件）。
- [x] **固定第三方 action 版本**：已钉到 commit SHA（见 `ci.yml` 注释）。
- [x] **重命名绕过**：`changed_files` / `diff_text` 使用 `--no-renames`。
- [x] **`test:map` 入 CI**。
- [ ] **阶段 2**：配置 bot 身份后 `APPROVALS=1 REQUIRE_CODEOWNERS=true` 再跑脚本。
- [ ] **阶段 3**：确认是否开启 L0 自动合并（默认否）。
- [ ] 核对 `rust-toolchain.toml` 1.100.0 与本地开发环境是否一致。
