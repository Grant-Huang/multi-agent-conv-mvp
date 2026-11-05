#!/bin/bash
# 启动 35B（Qwen3.8-35B-A3B）推理服务：5 个槽位，slot 0 留给前台对话，slot 1~4 给后台任务。
# 所有参数都可以用环境变量覆盖，路径不写死。
#
# 用法：
#   scripts/llama_server_35b.sh            # 前台运行（日志打到标准输出）
#   LLAMA_LOG=~/models/llama_8200.log scripts/llama_server_35b.sh   # 日志写文件
#
# 关键参数说明：
#   -np 5 + --kv-unified   5 个槽位共享同一个 KV 池（-c 是整个池子的大小，不是每槽位）
#   --cache-type-k/v q4_0  KV 缓存 4 位量化；V 缓存量化必须开 Flash Attention（-fa on）
#   --cache-ram            主机内存里的提示词缓存（多个对话的前缀复用），单位 MiB
#   --metrics              打开 /metrics（Prometheus 格式），排队数、生成速度等埋点来源
set -e

LLAMA_BIN="${LLAMA_BIN:-$HOME/tools/llama.cpp/build-master/bin/llama-server}"
MODELS_DIR="${MODELS_DIR:-$HOME/models}"
MODEL="${MODEL:-qwen3.8-35b-a3b/Qwen3.8-35B-A3B-Q4_K_M.gguf}"
PORT="${PORT:-8200}"
SLOTS="${SLOTS:-5}"
CTX="${CTX:-262144}"
CACHE_RAM="${CACHE_RAM:-8192}"
KV_TYPE="${KV_TYPE:-q4_0}"

if [ ! -x "$LLAMA_BIN" ]; then
    echo "找不到 llama-server：$LLAMA_BIN" >&2
    exit 1
fi

cd "$MODELS_DIR"

ARGS=(
    -m "./$MODEL"
    --port "$PORT"
    -np "$SLOTS"
    -c "$CTX"
    --kv-unified
    --cache-type-k "$KV_TYPE"
    --cache-type-v "$KV_TYPE"
    -fa on
    -ngl 99
    --cache-ram "$CACHE_RAM"
    --metrics
)

if [ -n "$LLAMA_LOG" ]; then
    exec "$LLAMA_BIN" "${ARGS[@]}" >> "$LLAMA_LOG" 2>&1
fi
exec "$LLAMA_BIN" "${ARGS[@]}"
