# Nightly 触发确定性方案（调度异常处置，简报 §4-B）

| 项目 | 内容 |
|---|---|
| 主线 | 第 2 线（夜间监视职责；2026-11-26 起从第 3 线移交，见[决定 2026-11-26-workstream-realignment](../decisions/2026-11-26-workstream-realignment.md)） |
| 来源 | #275 分诊指派：「调度异常（cron 延迟数小时）属于 CI（L3），按简报 §4 选项 B 先出方案」；背景见 [GR4_NIGHTLY_REVIEW_2026-11-28.md](GR4_NIGHTLY_REVIEW_2026-11-28.md) §3 |
| 状态 | **部分落地（2026-11-27）**——根因更正与当前形态见 §0；B2 watchdog 档保留为备选 |
| 改动面 | `.github/workflows/nightly-e2e.yml`（触发器）+ 新增 `.github/workflows/nightly-watchdog.yml`（轻量守门） |

## 0. 状态更新与根因更正（2026-11-27，#446 落地）

**根因更正**：本文 §1 第 1 条「GitHub 排队导致实际触发延迟」不准确。排队与派发是两层：每次运行的 `created_at` 与 `run_started_at` 完全相同 → **不是 runner 排队，是 GitHub schedule 事件本身派发得晚**（7 次运行实测延迟 7～12 小时，且每天都晚、无漏跑；工作流 11-19 起未改过触发配置，`concurrency` 不致延迟）。B1/B3 等仍依赖同一个不可靠定时器的方案据此降权。

**当前形态（2026-11-27 起生效，PR 见 #446 关联）**：
1. **主触发**：外部准点触发——每天 08:23 UTC 经 `workflow_dispatch` 拉起 `nightly-e2e`（2026-11-26 上线的 claude.ai 定时任务）；
2. **兜底**：cron 改非整点 `17 9 * * *`（09:17 UTC），与准点触发错开；
3. **守卫**：`nightly-e2e` 第一步检查过去 20 小时内是否已有成功/进行中运行（任一触发方式），命中即成功退出、不开失败 Issue——兜底晚到时不重复执行。

**其他更正**：失败自动开 Issue 的派工标签已由 `line:3` 改为 `line:2`（夜间监视 2026-11-26 移交第 2 线）；#275、#330 已在 11-26 夜间全量成功后结案。B2 独立 watchdog 工作流保留为备选（若外部准点触发不可用再启用）。

## 1. 问题（事实）

1. cron 仅 `30 10 * * *` 一档；实际触发延迟数小时（近五晚实测 17:43Z～22:10Z 不等，见简报 §2/§3）。~~根因初判为 GitHub 排队~~ **更正：schedule 事件派发延迟，见 §0**。
2. 「每晚」语义漂移：延迟跨日时，三晚计数口径（schedule 日 vs 触发墙钟日）不可判定。
3. **未触发无信号**：失败会自动开 Issue（#149 等），但「当晚根本没跑」没有任何机制告警（11-23 曾差一点整天无声）。

## 2. 目标

1. nightly 在预期窗口内**确定触发**（±数小时可接受，跨日不可接受）。
2. 超过截止未触发时**自动产生可见信号**（Issue 派第 2 线），与既有「失败自动开 Issue」对称。

## 3. 方案选项

| 方案 | 做法 | 优点 | 缺点 |
|---|---|---|---|
| B1 双 cron 档 | `30 10 * * *` + `0 16 * * *` 两档触发，`concurrency` 组保证只跑一次 | 改动最小（一行） | 仍是被动排队；两档都可能延迟，跨日风险降低但未消除 |
| **B2 watchdog（推荐）** | nightly 加 `workflow_dispatch` 触发器；新增每小时轻量 `nightly-watchdog.yml`：检查最近 26h 内有无 nightly run，无则 `workflow_dispatch` 主动触发 nightly，并开「未触发」Issue 派 `dispatch,line:3` | 触发确定性 + 未触发自动可见 + 与失败开 Issue 对称；watchdog 极轻（一次 API 查询） | 两个 workflow 文件；watchdog 用 `GH_TOKEN` 需最小权限（`actions: write`、`issues: write`） |
| B3 逐小时 cron + 守卫 | nightly 改每小时 cron，run 首步自检「26h 内已跑则跳过」 | 单文件 | 每小时唤醒 runner 队列，浪费；失败重试语义混乱 |
| C 保持现状 | 第 2 线人工监视 | 零改动 | 「未触发无信号」缺口保留 |

## 4. 推荐：B2（watchdog + workflow_dispatch）

伪码（watchdog 主体，实施时按 CI 组规范落）：

```yaml
# nightly-watchdog.yml（schedule: 每小时）
- name: 是否已有近期 nightly run？
  run: gh run list --workflow nightly-e2e.yml --created ">=$(date -u -d '26 hours ago' +%Y-%m-%dT%H:%M)" ...
- name: 未触发 → 补触发 + 开 Issue
  if: 无
  run: |
    gh workflow run nightly-e2e.yml          # 需 nightly-e2e.yml 增加 workflow_dispatch 触发器
    gh issue create --label "dispatch,line:3,nightly-e2e" \
      --title "nightly 未按时触发（自动 watchdog）" --body "原定 10:30Z 档位未在 26h 内运行，已补触发；请第 2 线复核计数口径。"
```

配套小改：`nightly-e2e.yml` 触发器 `[schedule, workflow_dispatch]`（约 1 行）。

## 5. 验收（实施卡的准出）

1. 反例：临时禁用 nightly 26h → watchdog 在 1h 内开「未触发」Issue 并补触发（演练一次留证据）；
2. 正例：watchdog 在 nightly 正常运行时**零动作**（连续 3 天无 issue）；
3. 权限最小化：watchdog token 仅 `actions: write` + `issues: write`；
4. 计数口径：随 GR-4 到期评审一并明确（简报 §5），watchdog 产生的补触发 run 标注 `dispatch` 来源。

## 6. 回滚

revert 两个 workflow 文件即可；watchdog 本身不触碰业务代码与数据。

## 7. 待批清单

- [ ] 方案批准（用户；L3）
- [ ] 实施卡归属：CI 组或授权第 2 线
- [ ] GR-4 评审时同步明确三晚计数口径
