#!/usr/bin/env bash
# World Adapter M0 集成合同套件：RT-WA HTTP + DES-UI/WABIND/WACHANNEL 浏览器。
# 用法：在仓库根目录执行 ./scripts/wa-m0-suite.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== RT-WA-01～09 (mom_world_mock) =="
CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_mock -- --nocapture

echo "== Playwright：DES* + WABIND + WACHANNEL =="
cd web
CHROME="${PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH:-}"
if [[ -z "$CHROME" ]]; then
  if [[ -x /usr/local/bin/google-chrome ]]; then
    CHROME=/usr/local/bin/google-chrome
  elif [[ -x /usr/bin/chromium ]]; then
    CHROME=/usr/bin/chromium
  fi
fi
export PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH="${CHROME:-}"
npx playwright test \
  e2e/des-experiment.spec.ts \
  e2e/mom-workbench.spec.ts \
  e2e/wa-integration.spec.ts

echo "== wa-m0 suite green =="
