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
    launchOptions:{executablePath},
    trace:'retain-on-failure',
    screenshot:'only-on-failure',
  },
  webServer: {
    command:'node e2e/harness.mjs',
    url:'http://127.0.0.1:18787/api/health',
    reuseExistingServer:false,
    timeout:30_000,
    gracefulShutdown:{signal:'SIGTERM',timeout:7000},
  },
});
