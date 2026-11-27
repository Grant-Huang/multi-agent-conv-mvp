#!/usr/bin/env bash
# 研发线定时领单：由系统定时任务（cron / launchd）每隔一段时间调用一次。
# 每次在该线的工作目录里，以非交互方式接着该目录最近的 Claude Code 会话跑一轮：
# 同步 master → 查派给本线的 Issue → 认领新单 → 推进手上的单 → 写进展。
#
# 用法：scripts/lines/line-tick.sh <线号> <该线工作目录>
# 说明与安装步骤见同目录 README.md；规则依据见
# docs/decisions/2026-11-27-line-tick-scheduler.md。
#
# 可选环境变量：
#   CLAUDE_BIN                 claude 可执行文件（默认 claude；cron 里 PATH 很短，建议写绝对路径）
#   LINE_TICK_HOME             锁和日志目录（默认 ~/.agentnexus-lines）
#   LINE_TICK_TIMEOUT_SEC      单次最长运行秒数，超时终止（默认 6000，即 100 分钟，小于 2 小时间隔）
#   LINE_TICK_PERMISSION_MODE  权限模式（默认 acceptEdits；账号支持 auto 时可设为 auto）
#   LINE_TICK_MODEL            指定模型（默认不指定，沿用会话设置）
#
# 超时、取消与清理：
#   - 存在 $LINE_TICK_HOME/line-<线号>.pause 时本轮跳过（人工对话期间用）；
#   - 用目录锁保证同一条线同一时刻只有一个 claude 在跑；上一轮没跑完，本轮直接跳过；
#   - 持锁进程已不存在（例如电脑重启）时，视为残留锁，清掉后照常运行；
#   - 超过 LINE_TICK_TIMEOUT_SEC 先发 TERM，30 秒后仍在则 KILL；
#   - 无论正常结束、出错还是被终止，退出时都释放锁。
set -uo pipefail

usage() { echo "用法：$0 <线号> <该线工作目录>" >&2; exit 2; }

N="${1:-}"; DIR="${2:-}"
[[ "$N" =~ ^[0-9]+$ ]] || usage
[ -n "$DIR" ] && [ -d "$DIR" ] || usage

CLAUDE_BIN="${CLAUDE_BIN:-claude}"
STATE="${LINE_TICK_HOME:-$HOME/.agentnexus-lines}"
TIMEOUT_SEC="${LINE_TICK_TIMEOUT_SEC:-6000}"
PERMISSION_MODE="${LINE_TICK_PERMISSION_MODE:-acceptEdits}"
mkdir -p "$STATE/logs"
LOG="$STATE/logs/line-$N-$(date +%Y%m%d).log"
LOCK="$STATE/line-$N.lock"

log() { echo "[$(date '+%F %T')] 第${N}线 $*" >> "$LOG"; }

# ---- 暂停开关：用户要直接和这条线对话时 touch 这个文件，聊完删掉 ----
if [ -e "$STATE/line-$N.pause" ]; then
  log "已暂停（存在 $STATE/line-$N.pause），本轮跳过"
  exit 0
fi

# ---- 加锁：mkdir 是原子操作，macOS 和 Linux 都可用（macOS 默认没有 flock） ----
if ! mkdir "$LOCK" 2>/dev/null; then
  holder="$(cat "$LOCK/pid" 2>/dev/null || true)"
  if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then
    log "上一轮（pid $holder）还在运行，本轮跳过"
    exit 0
  fi
  log "清理残留锁（pid ${holder:-未知} 已不存在）"
  rm -rf "$LOCK"
  mkdir "$LOCK" || { log "加锁失败"; exit 1; }
fi
echo $$ > "$LOCK/pid"

CPID=""; WPID=""
cleanup() {
  # 结束看门狗及其 sleep 子进程；若 claude 仍在（例如本脚本被中断），一并终止
  # （wait 收掉后台作业，避免 bash 打印 “Terminated” 混进 cron 邮件）
  [ -n "$WPID" ] && { pkill -P "$WPID"; kill "$WPID"; wait "$WPID"; } 2>/dev/null
  [ -n "$CPID" ] && kill "$CPID" 2>/dev/null
  rm -rf "$LOCK"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# ---- 本轮提示词：与 AGENTS.md“开始前”第 4 条、派工决策第 10～12 条一致 ----
PROMPT="这是系统定时任务触发的第${N}线例行领单（无人值守，不会有人回答你的提问）。按顺序做：
1. git fetch origin，把 origin/master 合进你当前的工作分支（不 rebase、不强推）；重读 AGENTS.md 中“开始前”“跨线分工”“禁止”三节。
2. 用 gh 列出标签 dispatch + line:${N}、状态 open 的 Issue。
3. 未标 queued 的单，不论严重度，凡是本线还没留言认领的，都在 8 小时内留言认领：写本线、计划、预计开始时间和预计 PR；排在后面的写明预计开始时间。标了 queued 的只看不领。
4. 按严重度从高到低，继续推进你已认领、还没完成的单：上一轮做到一半的先接着做完，不要另起炉灶。遵守在制品上限（每线待合并 PR 最多 3 个），完成的按“提交前自检”推送、开 PR（目标分支只能是 master，正文写 Closes #N）。
5. 本轮结束前，在你推进过的每张单上留一条进展（做到哪、下一步、卡在哪）；需要用户决定或批准的，在 Issue 里写清楚后停在那里，不要等待回复。
不合并 L1～L3 的 PR，不批准自己的改动，不改别人的分支；遇到 AGENTS.md 要求“先停下来问人”的事项，写进 Issue 后停下。"

ARGS=(-p --continue --permission-mode "$PERMISSION_MODE" --permission-prompts none)
[ -n "${LINE_TICK_MODEL:-}" ] && ARGS+=(--model "$LINE_TICK_MODEL")

log "开始（目录 $DIR，超时 ${TIMEOUT_SEC} 秒）"
cd "$DIR" || exit 2
"$CLAUDE_BIN" "${ARGS[@]}" "$PROMPT" >> "$LOG" 2>&1 &
CPID=$!

# ---- 看门狗：超时先 TERM，30 秒后仍在则 KILL ----
( sleep "$TIMEOUT_SEC"
  kill -TERM "$CPID" 2>/dev/null && echo "[$(date '+%F %T')] 第${N}线 超时，已终止" >> "$LOG"
  sleep 30; kill -KILL "$CPID" 2>/dev/null ) 2>/dev/null &
WPID=$!

rc=0; { wait "$CPID"; } 2>/dev/null || rc=$?
CPID=""
log "结束，退出码 $rc"
exit "$rc"
