import { defineConfig } from '@playwright/test';
import { existsSync } from 'node:fs';
const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH || (existsSync('/usr/bin/chromium') ? '/usr/bin/chromium' : undefined);
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: { timeout: 10_000 },
  workers: 1,
  retries: 0,
  outputDir: 'test-results/cases',
  reporter: [['list'], ['html', {open:'never'}], ['json', {outputFile:'test-results/results.json'}], ['junit',{outputFile:'test-results/junit.xml'}]],
  use: {
    baseURL:'http://127.0.0.1:18787',
    browserName:'chromium',
    viewport:{width:1440,height:1000},
    launchOptions:{
      executablePath,
      // T-75 E2E-ADMIC：假设备；ADMIC-01 仍可 stub getUserMedia 拒绝
      args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
    },
    trace:'retain-on-failure',
    screenshot:'only-on-failure',
  },
  webServer: {
    command:'node e2e/harness.mjs',
    // Q-T67-02：等 harness 深就绪门（/api/health + 首页数据接口 2s 内有响应），
    // 而不是只等 /api/health；未就绪时用例不启动，避免冷启动/高负载下的导航假红
    url:'http://127.0.0.1:18789/ready',
    reuseExistingServer:false,
    timeout:120_000,
    gracefulShutdown:{signal:'SIGTERM',timeout:7000},
  },
});
