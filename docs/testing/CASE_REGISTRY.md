# 用例登记改为“一用例一文件”（设计）

| 项目 | 内容 |
|---|---|
| 地位 | 用例登记格式与迁移的**权威设计**。实施由第 2 线负责（派工见决策） |
| 状态 | 设计已确认，未实施；实施前仍按现有 `docs/testing/features.json` 与 `docs/testing/CASES.md` 登记 |
| 来源 | 用户确认（2026-11-27：要求找出 PR 交叉冲突的解决方案；对助手方案的三项建议答复“3 个都可以”）。具体格式与步骤为助手建议、用户采纳 |
| 决策 | [2026-11-27-case-registry-per-file](../decisions/2026-11-27-case-registry-per-file.md) |

## 1. 问题

两个登记文件是全仓的冲突热点：

| 文件 | 规模（2026-11-27） | 过去 3 天被改动的提交数 |
|---|---|---|
| `docs/testing/features.json` | 1354 行；33 个特性的 JSON 数组；约 577 条用例引用 | 126 |
| `docs/testing/CASES.md` | 1165 行；约 76 个小节、约 595 行用例表 | 128 |

- 每个功能 PR 都要改这两个文件。只要有一个先合并，其余 PR 就会冲突。
- 2026-11-27 00:25 UTC，待合并的 8 个 PR 中有 6 个冲突：5 个冲突在 `features.json`，1 个在 `CASES.md` 加测试文件。
- `ai-desktop` 一个特性就有 132 条用例，所以只按特性拆文件也解决不了同一条线内部的冲突。

## 2. 新格式

```
docs/testing/
├── features/<特性 id>.yaml   特性元信息：document、sources、coverage（很少改）
├── cases/<用例 id>.yaml       一用例一文件：新增用例 = 新增一个文件
└── （生成，不入库）features.json、CASES.md
```

用例文件示例（字段与现有表格列一一对应，`group` 对应 CASES.md 的小节标题，`kind` 对应测试分类）：

```yaml
id: E2E-ADMEMW
feature: ai-desktop
kind: browser-e2e          # 见 fencing/INTEGRATION_PIPELINE.md 第 5.1 节的七类
group: AI 桌面：记忆
given: 打开 SMT-03 工作区
when: 纠正 mem-1 正文并保存；撤回 mem-2
then: 详情立即显示新正文；列表不再出现 mem-2；工作区草稿不丢
# 原表格有其他列时照原列名保留，例如 extra: {Case 备注: ...}
```

规则：

- 特性的用例清单由“`cases/` 下 `feature` 等于该特性的文件”推导，不在特性文件里重复列出（`DEV_STANDARD.md` 第 3.1 节：一件事只在一处描述）。
- 运行时用例（`RT-*`）与浏览器用例（`E2E-*`）同样处理，用 `kind` 区分。
- 改一个已有用例只改它自己的文件。两个 PR 同时改同一个用例才会冲突，这是真正需要人判断的语义冲突。

## 3. 检查与生成

- `web/verification/check-map.mjs`、`check-experience.mjs`（目前仅这两个程序读取登记文件）改为读取 `features/` 与 `cases/` 目录。
- 新增 `web/verification/build-registry.mjs`：
  - 从两个目录生成 `features.json` 与 `CASES.md`，供人阅读和兼容旧链接；
  - 输出到 `web/test-results/`（不入库），由 CI 作为产物上传；
  - 仓库里的 `docs/testing/CASES.md` 改为一页说明加生成命令。
- 新增检查：用例文件名与 `id` 一致；`feature` 必须存在于 `features/`；`id` 全仓唯一。

## 4. 迁移步骤（第 2 线）

1. 写转换脚本：把现有 `features.json` 与 `CASES.md` 拆成两个目录。转换前后核对：用例数、每个特性的用例集合、每条用例的各列文字完全一致；不一致的列出来人工处理。
2. 先写测试（Red）：用小样本目录验证新版 `check-map.mjs` / `check-experience.mjs` 的通过与失败情形，再改实现（Green）。
3. 选在制品较少的时间窗口一次性切换：转换、提交、合并。切换 PR 写明“在途 PR 请合并 master 后按新格式登记”，并提供 `node web/verification/migrate-pr-cases.mjs`，把分支里对旧文件的改动转成新文件。
4. 同步说法：
   - `AGENTS.md`“修改范围”与 `DEV_STANDARD.md` 第 3、3.1 节：用例登记位置改为 `docs/testing/cases/`（L3，需用户批准）；
   - `docs/testing/README.md`、`docs/GLOSSARY.md` 同步更新。

## 5. 过渡措施（切换前）

- 根目录 `.gitattributes` 给 `docs/testing/CASES.md` 设 `merge=union`。本地把 master 合进分支时，两边各自新增的行会自动都保留，减少手工解冲突。
  - 局限一：只对按行追加有效；两边改的是同一行时，两个版本都会保留，需要人删掉重复的那行。
  - 局限二：GitHub 网页上判断“能否合并”时是否认这个设置，**未核实**，所以只当临时措施。
- `features.json` 是 JSON 数组，不能用 union 合并，只能等切换。
- 切换完成后删除这条 `.gitattributes` 规则。

## 6. 与其他机制的关系

- **派工巡检**：每 4 小时列出有冲突的 PR 及冲突文件，在 PR 上提醒作者所在线处理。巡检和审查会话都不替各线合并或修改分支。
- 登记表以外的代码冲突，靠以下几项减少：
  - 变更范围声明（[集成流水线](../fencing/INTEGRATION_PIPELINE.md) 第 8 节）；
  - 在制品上限（第 3 节）；
  - 合并队列（第 4 节）。
