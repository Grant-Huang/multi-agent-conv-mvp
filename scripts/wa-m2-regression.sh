#!/usr/bin/env bash
# World Adapter M2：DES 闭环合同 + RT-DES/DESC 回归（不含 Playwright；E2E 见 M2-5）。
# 用法：在仓库根目录执行 ./scripts/wa-m2-regression.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== RT-WADES write（mom_world_des_write） =="
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des_write -- --nocapture

echo "== RT-WADES read（mom_world_des） =="
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture

echo "== RT-DES-08～11（an-runtime mom_des） =="
CARGO_TARGET_DIR=target cargo test --locked -p an-runtime --test mom_des -- \
  rt_des_08_selected_plan_b_changes_the_simulated_world \
  rt_des_09_post_execution_same_source_observation_is_verification_evidence \
  rt_des_10_new_disturbance_after_execution_still_invalidates_plan \
  rt_des_11_tool_transfer_failure_comes_from_the_simulation \
  -- --nocapture

echo "== RT-DESC connector（01～05、12） =="
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_des_connector -- --nocapture

echo "== wa-m2 regression green =="
