#!/usr/bin/env bash
# line-tick.sh 的自测：用假的 claude 程序代替真实 CLI，不联网、不花额度。
# 运行：bash scripts/lines/line-tick.test.sh
# 覆盖：参数与提示词、日志、并发时跳过、残留锁清理、超时终止、暂停开关、缺参数报错。
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
TICK="$HERE/line-tick.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILS=0

# 断言工具：条件不成立时打印说明并记一次失败
check() { # check <说明> <命令...>
  local desc="$1"; shift
  if "$@"; then echo "ok   - $desc"; else echo "FAIL - $desc"; FAILS=$((FAILS + 1)); fi
}

# 假 claude：把工作目录和全部参数写到 $FAKE_OUT，再按 $FAKE_SLEEP 秒“工作”
mkdir -p "$WORK/bin" "$WORK/line3"
cat > "$WORK/bin/claude" <<'FAKE'
#!/usr/bin/env bash
{ echo "cwd=$PWD"; for a in "$@"; do echo "arg=$a"; done; } > "$FAKE_OUT"
sleep "${FAKE_SLEEP:-0}"
FAKE
chmod +x "$WORK/bin/claude"
export CLAUDE_BIN="$WORK/bin/claude" LINE_TICK_HOME="$WORK/state" FAKE_OUT="$WORK/out1"

# 1. 正常一次：在该线目录运行、带 -p --continue、拒绝弹窗、提示词指向本线标签，写日志
FAKE_SLEEP=0 bash "$TICK" 3 "$WORK/line3"; rc=$?
check "正常运行退出码为 0" test "$rc" -eq 0
check "在该线工作目录中运行" grep -qx "cwd=$WORK/line3" "$WORK/out1"
check "非交互模式 -p" grep -qx "arg=-p" "$WORK/out1"
check "接着该目录最近的会话 --continue" grep -qx "arg=--continue" "$WORK/out1"
check "无人值守时拒绝所有弹窗" grep -qx "arg=none" "$WORK/out1"
check "提示词包含本线标签 line:3" grep -q "line:3" "$WORK/out1"
check "提示词要求 8 小时内认领" grep -q "8 小时内" "$WORK/out1"
check "写入本线日志" bash -c "ls '$WORK/state/logs/' | grep -q '^line-3-'"
check "运行结束后释放锁" test ! -e "$WORK/state/line-3.lock"

# 2. 并发：第一次还在跑时，第二次直接跳过，不启动第二个 claude
export FAKE_OUT="$WORK/out2"
FAKE_SLEEP=3 bash "$TICK" 3 "$WORK/line3" & first=$!
sleep 1
FAKE_OUT="$WORK/out3" bash "$TICK" 3 "$WORK/line3"; rc=$?
check "并发时第二次退出码为 0（跳过不算错）" test "$rc" -eq 0
check "并发时第二次不启动 claude" test ! -e "$WORK/out3"
wait "$first"

# 3. 残留锁：持锁进程已不存在时，清掉锁照常运行
mkdir -p "$WORK/state/line-3.lock"; echo 999999 > "$WORK/state/line-3.lock/pid"
FAKE_OUT="$WORK/out4" FAKE_SLEEP=0 bash "$TICK" 3 "$WORK/line3"
check "残留锁被清理后照常运行" test -e "$WORK/out4"

# 4. 超时：claude 卡住超过上限时被终止，退出码非 0，锁被释放
start=$(date +%s)
FAKE_OUT="$WORK/out5" FAKE_SLEEP=30 LINE_TICK_TIMEOUT_SEC=2 bash "$TICK" 3 "$WORK/line3"; rc=$?
took=$(( $(date +%s) - start ))
check "超时后退出码非 0" test "$rc" -ne 0
check "超时后 10 秒内结束（实际 ${took} 秒）" test "$took" -lt 10
check "超时后释放锁" test ! -e "$WORK/state/line-3.lock"

# 5. 暂停开关：存在 line-3.pause 时跳过，不启动 claude
touch "$WORK/state/line-3.pause"
FAKE_OUT="$WORK/out6" bash "$TICK" 3 "$WORK/line3"; rc=$?
check "暂停时退出码为 0" test "$rc" -eq 0
check "暂停时不启动 claude" test ! -e "$WORK/out6"
rm "$WORK/state/line-3.pause"

# 6. 参数缺失或目录不存在：退出码 2
bash "$TICK" 3 >/dev/null 2>&1; rc=$?
check "缺少目录参数时退出码 2" test "$rc" -eq 2
bash "$TICK" 3 "$WORK/nope" >/dev/null 2>&1; rc=$?
check "目录不存在时退出码 2" test "$rc" -eq 2
bash "$TICK" x "$WORK/line3" >/dev/null 2>&1; rc=$?
check "线号不是数字时退出码 2" test "$rc" -eq 2

echo "失败 $FAILS 项"
exit $(( FAILS > 0 ))
