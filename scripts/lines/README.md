# 研发线定时领单（系统定时任务驱动）

| 项目 | 内容 |
|---|---|
| 地位 | 各线“自动查单、认领、推进”的**运行方式**的权威说明；规则本身见 `AGENTS.md`“开始前”第 4 条与[派工决策](../../docs/decisions/2026-11-25-dispatch-to-workstreams.md)第 10～12 条 |
| 决策 | [2026-11-27-line-tick-scheduler](../../docs/decisions/2026-11-27-line-tick-scheduler.md) |
| 适用 | 在用户本机（macOS / Linux）的终端或 Cursor 里运行的研发线。Windows 需在 WSL 中使用（未验证） |

## 1. 它解决什么

原来各线在自己的会话里跑 `/loop 2h`。这种循环只活在那个会话里：关掉终端、Cursor 重载、电脑重启就没了；会话一直忙着做大单时也不会触发。结果有的线领单、有的不领，领了也会停在半路。

现在改由**电脑的定时任务**每 2 小时调用一次 `line-tick.sh`。它在该线的目录里以非交互方式**接着该目录最近的会话**跑一轮：同步 master → 查单 → 认领 → 接着做上一轮没做完的单 → 在单上写进展。终端开没开、会话有没有重启，都不影响下一轮。

## 2. 一次性准备（每条线做一次）

1. **每条线一个独立目录**（建议用 `git worktree`，不要几条线共用一个目录，因为 `--continue` 按目录找会话）：

   ```bash
   git worktree add ~/lines/line3 -b line3-work origin/master
   ```

2. **在该目录里交互地跑一次 `claude`**，确认 GitHub 命令行 `gh` 已登录、能正常查单。之后定时任务会接着这次的会话。

3. **给无人值守的运行放行常用命令**。定时运行时没有人点“允许”，未放行的命令一律被拒绝（脚本用了 `--permission-prompts none`，不会卡住等人）。在该目录的 `.claude/settings.local.json`（已被 `.gitignore` 忽略）写：

   ```json
   {
     "permissions": {
       "allow": [
         "Bash(git *)", "Bash(gh *)", "Bash(cargo *)", "Bash(npm *)",
         "Bash(node *)", "Bash(python3 *)", "Bash(ls *)", "Bash(cat *)", "Bash(grep *)"
       ],
       "deny": ["Bash(git push --force*)", "Bash(git push -f*)", "Bash(git rebase*)", "Bash(gh pr merge*)", "Bash(gh api*)"]
     }
   }
   ```

   `deny` 里的前三条对应 `AGENTS.md`“跨线分工”：解冲突只合并 master，不 rebase、不强推。**2026-11-27 起（决策 `2026-11-27-unattended-merge-and-tool-gates`）：无人值守任务禁 `gh pr merge` 与 `gh api`——包括 L0 在内的所有 PR 合并一律由人工批准**；无人值守线在 PR 留言请用户合并即可（合并队列 #433 只入队、催批、关卡）。第 6 线如需启动服务，再按需加。

4. **试跑一次**，看日志：

   ```bash
   scripts/lines/line-tick.sh 3 ~/lines/line3
   ```

   ```bash
   tail -n 50 ~/.agentnexus-lines/logs/line-3-$(date +%Y%m%d).log
   ```

## 3. 定时（crontab）

`crontab -e` 加入以下内容（时间是本机时间；各线错开 10 分钟，避开整点与半点）。把仓库路径、各线目录和 `claude` 的绝对路径（`which claude` 查）换成你自己的：

```
PATH=/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin
CLAUDE_BIN=/opt/homebrew/bin/claude
TICK=/Users/你/multi-agent-conv-mvp/scripts/lines/line-tick.sh
7  */2 * * * $TICK 1 /Users/你/lines/line1
17 */2 * * * $TICK 2 /Users/你/lines/line2
27 */2 * * * $TICK 3 /Users/你/lines/line3
37 */2 * * * $TICK 4 /Users/你/lines/line4
47 */2 * * * $TICK 5 /Users/你/lines/line5
57 */2 * * * $TICK 6 /Users/你/lines/line6
```

注意：

- **电脑睡眠时 cron 不运行**，醒来后也不补跑；需要整夜运行时让电脑保持唤醒（macOS 可在终端运行 `caffeinate -s`）。
- macOS 首次使用 cron 可能要在“系统设置 → 隐私与安全性 → 完全磁盘访问权限”里允许 `cron`。
- 每条线每天最多 12 轮，每轮都消耗用量。额度紧时把 `*/2` 改成 `*/4`。

## 4. 日常操作

| 想做什么 | 怎么做 |
|---|---|
| 直接和某条线对话 | 先 `touch ~/.agentnexus-lines/line-3.pause` 暂停它的定时领单，再到该目录运行 `claude --continue`；聊完退出并删掉这个文件 |
| 看某条线最近在干什么 | 看 `~/.agentnexus-lines/logs/line-N-日期.log`，或看它在 Issue 上留的进展 |
| 某一轮卡住 | 单轮超过 100 分钟会被自动终止，下一轮接着做；也可以改 `LINE_TICK_TIMEOUT_SEC` |
| 停掉一条线 | 删掉 crontab 中那一行，或长期放着 pause 文件 |

## 5. 脚本行为与自测

- 同一条线同一时刻只跑一个；上一轮没结束，本轮跳过；电脑重启留下的残留锁会被自动清理。
- 每一轮的提示词写在 `line-tick.sh` 中，只列出本轮步骤，并要求先重读 `AGENTS.md`；规则以 `AGENTS.md` 为准。规则改了而步骤不变时，不需要改脚本。
- 自测（假的 `claude`，不联网、不消耗用量）：

  ```bash
  bash scripts/lines/line-tick.test.sh
  ```
