import { test, expect, type APIRequestContext } from '@playwright/test';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// UXFIX-B（#305，INTENT-003，CH-U5）：测试模式关闭（服务端 desktop.test_fixtures=false）
// 时，服务端在快照里告知 false，正常界面的测试控件数量为 0（本用例覆盖设置面板的
// “注入冲突（测试）”；工作区模拟按钮与证据面板身份切换由 WorkspaceView/EvidencePanel
// 的组件级测试覆盖——生产模式无夹具任务，无法经界面打开工作区）。
// 开启时现有 E2E（desktop.spec.ts 等）不变。与 ops.spec.ts 同样的自管服务器方式
//（端口 18795，不复用 harness）。

const port = 18795;
const on = `http://127.0.0.1:${port}`;
const root = fileURLToPath(new URL('../../', import.meta.url));

let child: ReturnType<typeof spawn> | undefined;
let logs = '';

async function startProdServer(): Promise<void> {
  const dir = await mkdtemp(path.join(tmpdir(), 'an-prod-mode-'));
  await mkdir(path.join(dir, 'workspace'), { recursive: true });
  await mkdir(path.join(dir, 'logs'), { recursive: true });
  const config = path.join(dir, 'config.toml');
  // 无 [desktop] 段：test_fixtures 缺省 false（生产形态）；快照应告知 false
  await writeFile(config, [
    '[server]', `bind="127.0.0.1:${port}"`, `db_path=${JSON.stringify(path.join(dir, 'app.db'))}`,
    `static_dir=${JSON.stringify(path.join(root, 'web/dist'))}`,
    '[llm]', 'base_url="http://127.0.0.1:1/v1"', 'model="unused"', 'max_retries=0', 'timeout_secs=2',
    '[agents.general]', `workspace_dir=${JSON.stringify(path.join(dir, 'workspace'))}`, 'websearch_command=""',
    '[agents.coding]', 'enabled=false',
    '[acp]', 'auto_detect=false',
    '[nodes]', 'discovery=false',
    '[logging]', `dir=${JSON.stringify(path.join(dir, 'logs'))}`,
  ].join('\n'));
  logs = '';
  child = spawn(path.join(root, 'target/debug/agentnexus'), [config], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout?.on('data', c => { logs += c.toString(); });
  child.stderr?.on('data', c => { logs += c.toString(); });
  const handle = child;
  await expect.poll(async () => {
    if (handle.exitCode !== null) throw Error(logs);
    try { return (await fetch(on + '/api/health')).status; } catch { return 0; }
  }, { timeout: 30_000 }).toBe(200);
}

async function stopServer() {
  const handle = child;
  child = undefined;
  if (handle && handle.exitCode === null) {
    await new Promise<void>(resolve => { const t = setTimeout(() => handle.kill('SIGKILL'), 7000); handle.once('exit', () => { clearTimeout(t); resolve(); }); handle.kill('SIGINT'); });
  }
}

test.beforeEach(async () => { await startProdServer(); });
test.afterEach(async () => { await stopServer(); });

const getJson = async (request: APIRequestContext, p: string) => (await (await request.get(`${on}${p}`)).json()) as unknown;

test('E2E-ADTESTCTRL: 测试模式关闭时快照告知 false 且设置面板无测试控件', async ({ page, request }) => {
  // 服务端告知：快照 test_fixtures=false（UXFIX-B 的服务端口径）
  const snap = (await getJson(request, '/api/desktop/v1/snapshot')) as { test_fixtures?: boolean };
  expect(snap.test_fixtures).toBe(false);

  // 打开「设置与管理」→ 助理：无“注入冲突（测试）”
  await page.goto(on + '/');
  await page.getByTestId('open-settings').click();
  await expect(page.getByTestId('settings-panel')).toBeVisible();
  await expect(page.getByTestId('profile-inject-conflict')).toHaveCount(0);
  await expect(page.getByTestId('settings-panel')).not.toContainText('注入冲突（测试）');
});
