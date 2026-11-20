# 提交前自检（Preflight）

给 AI 编码 agent（与开发者）在 **push / 开 PR / 声称完成** 之前跑的清单。
上位入口：`AGENTS.md`「提交前自检」。门禁含义见 `docs/CI_GATES.md`。

在仓库根目录执行。基线默认 `origin/master`。

## 1. 范围与风险（先看再跑）

```bash
git status
git diff --stat origin/master
python3 tools/ci/risk_classify.py --base origin/master
```

- 风险为 **L2/L3**，或改动触及 `AGENTS.md`「先停下来问人」路径：须已有方案且用户批准，再继续。
- 单次 PR 尽量一个目标；跨很多组件时考虑拆分，或准备 `## 大改动说明`（≥ 40 字）。

## 2. 与 CI 对齐的命令（须贴真实输出）

```bash
CARGO_TARGET_DIR=target cargo fmt --all -- --check
CARGO_TARGET_DIR=target cargo clippy --locked --all-targets -- -D warnings
CARGO_TARGET_DIR=target cargo test --locked --workspace
# 若改了 an-core 导出类型或 web 用到的接口：
CARGO_TARGET_DIR=target cargo test --locked -p an-core
git diff --exit-code -- web/src/bindings

cd web && npm ci && npm run typecheck && npm test && npm run test:map && npm run build && cd ..
```

只改了 `tools/ci` 时额外：

```bash
cd tools/ci && python3 -m pytest -q && cd ../..
# 有 flake8 时：
# python3 -m flake8 --max-line-length 100 tools/ci
```

只改了 `tools/websearch` / `tools/dlp-egress` 时，按各目录 README / pyproject 跑对应 pytest。

## 3. 变更策略（会阻断合并的项）

用与 CI 相同的脚本；`PR_BODY` 用你准备写进 PR 的正文（可先写入临时文件）：

```bash
PR_BODY="$(cat <<'EOF'
## 非目标
不修改任务状态机与公开 HTTP API。

## 关联
Case ID：RT-…

## 验证证据
（贴命令输出摘要）
EOF
)"
export PR_BODY

python3 tools/ci/change_size_check.py --base origin/master
python3 tools/ci/injection_scan.py --base origin/master
```

| 结果 | 怎么处理 |
|---|---|
| 规模 **block** | 拆 PR，或加 `## 大改动说明`（≥ 40 字） |
| 测试删除 **block** | 恢复测试，或加 `## 测试删除说明`（≥ 20 字） |
| 测试弱化 **block** | 不要 ignore/skip 糊弄；确需改断言则加 `## 测试变更说明`（≥ 20 字） |
| 注入扫描 **block** | 改掉触发内容，或按规则加 `## 注入扫描说明` |
| 非文档缺非目标 / Case ID | **warn**：补上后再开 PR |

弱化检测只扫测试路径；不要在测试文件里用字面量 `.skip(` / `#[ignore]` 写样例字符串（会自触发），样例用运行时拼接。

## 4. PR 描述最低要求

对照 `.github/pull_request_template.md`：

- [ ] **变更说明**（做什么、为什么）
- [ ] **关联**：Case / Feature ID（或写明「基线补录」）
- [ ] **非目标**（≥ 约 8 字，写清不改什么）
- [ ] **风险与回滚**（等级以 `risk_classify` 为准）
- [ ] **验证证据**：真实命令与结果行，禁止只写「已通过」
- [ ] 需要时才加：大改动说明 / 测试删除说明 / 测试变更说明 / 注入扫描说明

## 5. 禁止项（提交前再确认）

- 不直接推 `master` / `main`；走 PR。
- 不批准自己的 PR；不绕过 CI。
- 不删改既有测试断言来让红变绿；不无故 `#[ignore]` / `.skip` / `.only`。
- 不把密钥、令牌写进仓库或日志。

## 6. 完成后

在回复里贴出第 2、3 节命令的**实际输出**（可截关键行），再 push / 开 PR。
