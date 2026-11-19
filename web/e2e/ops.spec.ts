import { test, expect, type Page, type APIRequestContext } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// 运营桌面与运营频道（docs/mom/OPS_DESK.md，DECISIONS 56–58）。
// 开启 [mom] chat_bridge 的实例每个用例单独启动在 18791（避免卡片、小时上限在用例间累积）；
// E2E-OPSOFF 使用默认 harness（18787，未配置 [mom]，开关默认关闭）。
const root = fileURLToPath(new URL('../../', import.meta.url));
const on = 'http://127.0.0.1:18791';
const CHANNEL = 'ops-channel';
let dist = '';
let work = '';
let child: ChildProcess | undefined;
let logs = '';

test.beforeAll(async () => {
  test.setTimeout(300_000);
  execFileSync('cargo', ['build', '--locked', '--bin', 'agentnexus'], { cwd: root, env: { ...process.env, CARGO_TARGET_DIR: 'target' }, stdio: 'pipe' });
  work = mkdtempSync(path.join(tmpdir(), 'ops-browser-'));
  dist = path.join(work, 'dist');
  // 前端构建到临时目录，不覆盖默认 harness 正在使用的 web/dist
  execFileSync('npx', ['vite', 'build', '--outDir', dist, '--emptyOutDir'], { cwd: path.join(root, 'web'), stdio: 'pipe' });
});

async function startServer(name: string) {
  const dir = mkdtempSync(path.join(work, `${name}-`));
  const workspace = path.join(dir, 'workspace');
  mkdirSync(workspace, { recursive: true });
  const config = path.join(dir, 'config.toml');
  // 模型地址指向不可用端口：运营频道若误触发模型回复，会在界面或任务里留下痕迹
  writeFileSync(config, [
    '[server]', 'bind="127.0.0.1:18791"', `db_path=${JSON.stringify(path.join(dir, 'app.db'))}`, `static_dir=${JSON.stringify(dist)}`,
    '[llm]', 'base_url="http://127.0.0.1:1/v1"', 'model="unused"', 'max_retries=0', 'timeout_secs=2',
    '[agents.general]', `workspace_dir=${JSON.stringify(workspace)}`, 'websearch_command=""',
    '[agents.coding]', 'enabled=false',
    '[acp]', 'auto_detect=false',
    '[nodes]', 'discovery=false',
    '[mom]', 'chat_bridge=true', 'ops_merge_secs=1', 'ops_hourly_cap=10',
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
  const out = path.join(root, 'web/test-results/ops');
  mkdirSync(out, { recursive: true });
  writeFileSync(path.join(out, 'server.log'), logs);
}
test.beforeEach(async () => {
  if (!test.info().title.startsWith('E2E-OPSOFF')) await startServer(test.info().title.slice(0, 12).replace(/\W/g, ''));
});
test.afterEach(async () => { await stopServer(); });

const mom = (p: string) => `${on}/mom/api/mom${p}`;
const unwrap = (j: any) => j.data ?? j;
async function scenario(request: APIRequestContext): Promise<string> {
  const r = await request.post(mom('/scenarios/s1'));
  expect(r.ok(), await r.text()).toBeTruthy();
  return unwrap(await r.json()).case_id;
}
async function command(request: APIRequestContext, id: string, body: object) {
  const r = await request.post(mom(`/cases/${id}/commands`), { data: body });
  expect(r.ok(), await r.text()).toBeTruthy();
}
// 触发安全联锁：案例进入受阻，引擎发出 SafetyBlocked
async function trip(request: APIRequestContext, batch: string, n = 0) {
  return request.post(mom('/events'), {
    data: { id: `${batch}-trip-${n}`, scenario: 's1', kind: 'vibration', source: 'EAM', occurred_at: 900 + n, batch, customer: 'A', asset: 'CNC-XX', payload: { line: 'Line-B', safety_trip: true } },
  });
}
async function seedBlocked(request: APIRequestContext, batch: string) {
  const r = await trip(request, batch);
  expect(r.ok(), await r.text()).toBeTruthy();
}
async function seedClosed(request: APIRequestContext) {
  const id = await scenario(request);
  const view = unwrap(await (await request.get(mom(`/cases/${id}`))).json());
  await command(request, id, { command: 'approve', scope: 'production', plan_revision: view.plan_revision, actor: 'supervisor' });
  await command(request, id, { command: 'execute', scope: 'production' });
  await command(request, id, { command: 'advance', minutes: 5 });
  await command(request, id, { command: 'close' });
  return id;
}
async function openDesk(page: Page) {
  await page.goto(on + '/');
  await page.getByRole('tab', { name: /运营桌面/ }).click();
}
const strip = (page: Page) => page.getByTestId('ops-strip');
const rows = (page: Page) => page.getByTestId('ops-row');
async function openChannel(page: Page) {
  await page.goto(on + '/');
  await page.getByRole('complementary', { name: '会话列表' }).getByRole('button', { name: /运营频道/ }).click();
}
const channelMessages = async (request: APIRequestContext) => (await (await request.get(`${on}/api/conversations/${CHANNEL}/messages`)).json()) as unknown[];

test('E2E-OPSDESK: 桌面按风险排序，待批准置顶，已完成折叠，数据标模拟，桌面不能批准', async ({ page, request }) => {
  const pending = await scenario(request);
  await seedBlocked(request, 'OPS-BLOCKED-1');
  const closed = await seedClosed(request);
  await openDesk(page);
  // 厂况条数字与事项一致
  await expect(strip(page)).toContainText('待批准 1');
  await expect(strip(page)).toContainText('受阻 1');
  // 待批准置顶，其次受阻
  await expect(rows(page).first()).toContainText(pending);
  await expect(rows(page).nth(1)).toContainText('受阻');
  // 已完成默认折叠，但保留总数；已完成案例不在可见事项行中
  await expect(page.getByRole('button', { name: /已完成.*1/ })).toBeVisible();
  await expect(rows(page).filter({ hasText: closed })).toHaveCount(0);
  // 每行有“模拟”标识，且只有一个主操作
  for (const row of await rows(page).all()) {
    await expect(row).toContainText('模拟');
    await expect(row.getByRole('button')).toHaveCount(1);
  }
  await expect(rows(page).first().getByRole('button', { name: '在频道中处理' })).toBeVisible();
  // 桌面没有批准入口
  await expect(page.getByRole('button', { name: '批准', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '批准执行' })).toHaveCount(0);
  // 筛选选择在刷新后保持
  await page.getByRole('button', { name: '待我批准' }).click();
  await expect(page.getByRole('button', { name: '待我批准' })).toHaveAttribute('aria-pressed', 'true');
  await page.reload();
  await page.getByRole('tab', { name: /运营桌面/ }).click();
  await expect(page.getByRole('button', { name: '待我批准' })).toHaveAttribute('aria-pressed', 'true');
});

test('E2E-OPSDRAWER: 详情抽屉按六个业务问题展示，原始数据折叠，关闭后回到触发位置', async ({ page, request }) => {
  const pending = await scenario(request);
  await openDesk(page);
  const row = rows(page).filter({ hasText: pending }).first();
  await row.click();
  const drawer = page.getByRole('dialog', { name: '事项详情' });
  await expect(drawer).toBeVisible();
  for (const h of ['发生了什么与影响', '责任', '方案与前提', '缺什么', '进展与下次反馈', '凭什么算解决']) {
    await expect(drawer.getByRole('heading', { name: h })).toBeVisible();
  }
  // 原始数据默认折叠
  await expect(drawer.getByText('projection_seq')).toBeHidden();
  await drawer.getByText('原始数据').click();
  await expect(drawer.getByText('projection_seq')).toBeVisible();
  // 抽屉里也没有批准按钮
  await expect(drawer.getByRole('button', { name: '批准', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();
  await expect(row).toBeFocused();
});

test('E2E-OPSCHANNEL: P0 事件在运营频道只出一张卡并就地更新，未读计数，无模型回复无任务', async ({ page, request }) => {
  const id = await scenario(request);
  await page.goto(on + '/');
  await expect(page.getByTestId('ops-unread')).toHaveText('1');
  await openChannel(page);
  const cards = page.getByTestId('ops-card').filter({ hasText: id });
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toContainText('模拟');
  // 打开频道后未读清零
  await expect(page.getByTestId('ops-unread')).toHaveCount(0);
  // 同一案例状态变化（被批准）：仍是一张卡，内容更新，频道消息条数不变
  const before = (await channelMessages(request)).length;
  const view = unwrap(await (await request.get(mom(`/cases/${id}`))).json());
  await command(request, id, { command: 'approve', scope: 'production', plan_revision: view.plan_revision, actor: 'supervisor' });
  await expect(cards.first()).toContainText('supervisor');
  await expect(cards).toHaveCount(1);
  expect((await channelMessages(request)).length).toBe(before);
  // 不触发模型回复，不创建任务
  await expect(page.locator('.msg.assistant:not([data-testid="ops-card"])')).toHaveCount(0);
  const tasks = await (await request.get(`${on}/api/tasks?conversation_id=${CHANNEL}`)).json();
  expect(tasks).toHaveLength(0);
});

test('E2E-OPSNOISE: 超过小时上限合并成汇总卡，同案例重复观测不新增消息，桌面保留全部', async ({ page, request }) => {
  // 先让一个案例有卡，再对它重复上报观测：只更新原卡，不新增频道消息
  const same = 'OPS-NOISE-SAME';
  await seedBlocked(request, same);
  await expect.poll(async () => (await channelMessages(request)).length).toBe(1);
  await page.waitForTimeout(2500);
  for (let i = 1; i <= 10; i++) await trip(request, same, i);
  await page.waitForTimeout(2500);
  expect((await channelMessages(request)).length).toBe(1);
  // 再来 12 个新案例：连同上面一个共 13 个，每小时上限 10 张，其余 3 个并入汇总卡
  const ids: string[] = [];
  for (let i = 0; i < 12; i++) ids.push(await scenario(request));
  await openChannel(page);
  await expect(page.getByTestId('ops-card')).toHaveCount(10);
  await expect(page.getByTestId('ops-summary')).toContainText('另有 3 条');
  // 桌面仍能看到全部 13 个事项，没有静默丢弃
  await page.getByRole('tab', { name: /运营桌面/ }).click();
  await expect(rows(page)).toHaveCount(13);
  for (const id of ids) await expect(rows(page).filter({ hasText: id })).toHaveCount(1);
});

test('E2E-OPSOFF: 关闭开关（默认）时没有桌面页签和运营频道，原有会话行为不变', async ({ page, request }) => {
  // 默认 harness（18787）没有配置 [mom]，chat_bridge 默认关闭；MOM 事件照常可以注入
  const r = await request.post('/mom/api/mom/scenarios/s1');
  expect(r.ok()).toBeTruthy();
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /运营桌面/ })).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: '会话列表' }).getByRole('button', { name: /运营频道/ })).toHaveCount(0);
  expect((await request.get('/api/mom/desk')).status()).toBe(404);
  const convs = await (await request.get('/api/conversations')).json();
  expect(convs.some((c: { id: string }) => c.id === CHANNEL)).toBe(false);
  await page.getByRole('button', { name: '＋ 新会话', exact: true }).click();
  await page.getByRole('textbox', { name: '输入消息' }).fill('你好验证');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  await expect(page.locator('.msg.assistant').last()).toContainText('你好，验证成功');
});
