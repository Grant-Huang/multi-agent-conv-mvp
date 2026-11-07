import { test, expect } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root=fileURLToPath(new URL('../../',import.meta.url));
const base='http://127.0.0.1:18790';
let processHandle:ChildProcess;
let logs='';
test.beforeAll(async()=>{
  test.setTimeout(180_000);
  execFileSync('cargo',['build','--locked','--bin','an-mom'],{cwd:root,env:{...process.env,CARGO_TARGET_DIR:'target'},stdio:'pipe'});
  const dir=mkdtempSync(path.join(tmpdir(),'mom-browser-'));
  processHandle=spawn(path.join(root,'target/debug/an-mom'),['--db',path.join(dir,'mom.db'),'--bind','127.0.0.1:18790'],{cwd:root,stdio:['ignore','pipe','pipe']});
  processHandle.stdout?.on('data',chunk=>{logs+=chunk.toString()});
  processHandle.stderr?.on('data',chunk=>{logs+=chunk.toString()});
  await expect.poll(async()=>{
    if(processHandle.exitCode!==null)throw Error(logs);
    try{return (await fetch(base+'/api/mom/cases')).status}catch{return 0}
  },{timeout:20_000}).toBe(200);
  const evidence=path.join(root,'web/test-results/mom-runtime');mkdirSync(evidence,{recursive:true});
  writeFileSync(path.join(evidence,'state.json'),JSON.stringify({base,database:path.join(dir,'mom.db'),provenance:'mock'},null,2));
});
test.afterAll(async()=>{
  if(processHandle&&processHandle.exitCode===null){
    await new Promise<void>(resolve=>{const timer=setTimeout(()=>{processHandle.kill('SIGKILL');},7000);processHandle.once('exit',()=>{clearTimeout(timer);resolve()});processHandle.kill('SIGINT');});
  }
  const dir=path.join(root,'web/test-results/mom-runtime');mkdirSync(dir,{recursive:true});writeFileSync(path.join(dir,'server.log'),logs);
});

test('E2E-MOM: production approval and shared board',async({page})=>{
  await page.goto(base);
  await page.getByRole('button',{name:'＋ 多扰动 S1'}).click();
  await expect(page.locator('#content h2').first()).toContainText('WAITING_HUMAN');
  await expect(page.locator('#content')).toContainText('CNC-XX');
  await page.getByRole('button',{name:'Agent 机器视图'}).click();
  await expect(page.locator('#content')).toContainText('next_cursor');
  await expect(page.locator('#content')).toContainText('Never bypass safety');
  await page.getByRole('button',{name:'人类运营视图'}).click();
  await page.getByRole('button',{name:'批准',exact:true}).click();
  await expect(page.locator('#content')).toContainText('supervisor');
  await page.getByRole('button',{name:'执行已批准动作'}).click();
  await expect(page.locator('#content h2').first()).toContainText('VERIFYING');
  await page.getByRole('button',{name:'推进 5 分钟'}).click();
  await expect(page.locator('#content')).toContainText('mock-verification');
  await page.getByRole('button',{name:'核验并关闭案例'}).click();
  await expect(page.locator('#content h2').first()).toContainText('CLOSED');
  await page.screenshot({path:test.info().outputPath('mom-production.png'),fullPage:true});
});

test('E2E-COMPLAINT: initial response before root cause and incomplete 8D',async({page})=>{
  await page.goto(base);
  await page.getByRole('button',{name:'＋ 客诉 S2'}).click();
  await expect(page.locator('#content h2').first()).toContainText('WAITING_HUMAN');
  await expect(page.locator('#content')).toContainText('INITIAL_RESPONSE');
  await expect(page.locator('#content')).toContainText('UNKNOWN');
  await expect(page.locator('#content')).toContainText('PROGRESS_8D');
  const approve=page.getByRole('button',{name:'批准',exact:true});await approve.first().click();
  await expect(page.locator('#content')).toContainText('supervisor');
  await page.getByRole('button',{name:'执行已批准动作'}).first().click();
  await expect(page.locator('#content h2').first()).toContainText('INVESTIGATING');
  await expect(page.locator('#content')).toContainText('customer.send');
  await page.getByRole('button',{name:'核验并关闭案例'}).click();
  await expect(page.locator('#error')).toContainText('verification incomplete');
  await expect(page.locator('#content h2').first()).not.toContainText('CLOSED');
  await page.screenshot({path:test.info().outputPath('mom-complaint.png'),fullPage:true});
});

test('E2E-MOMDYNAMIC: main entry and configurable continuous simulation',async({page})=>{
  await page.goto('/');
  await page.getByRole('link',{name:'MOM 运营沙盘'}).click();
  await expect(page).toHaveURL(/\/mom\/$/);
  await expect(page.getByRole('heading',{name:'MOM 运营公告板'})).toBeVisible();
  await page.getByText('配置持续仿真',{exact:true}).click();
  await page.getByLabel('订单数量').fill('900');await page.getByLabel('随机种子').fill('42');
  await page.getByLabel('持续事件数量').fill('2');await page.getByLabel('事件间隔（仿真分钟）').fill('10');
  await page.getByRole('button',{name:'创建配置仿真'}).click();
  await expect(page.locator('#content h2').first()).toContainText('COLLECTING');
  await expect(page.locator('#content')).toContainText('M01');
  await expect(page.locator('#content')).toContainText('MES/QMS/EAM/WMS');
  for(let i=0;i<4;i++){await page.getByRole('button',{name:'推进 5 分钟'}).click();await expect(page.locator('#error')).toBeEmpty();}
  await expect(page.getByRole('heading',{name:'排产方案与影响'}).locator('..')).toContainText('"quantity": 900');
  await page.getByRole('button',{name:'播放 30 分钟'}).click();await expect(page.locator('#playback-state')).toContainText('播放中');
  await page.getByRole('button',{name:'暂停仿真'}).click();await expect(page.locator('#playback-state')).toContainText('已暂停');
  await page.screenshot({path:test.info().outputPath('mom-dynamic.png'),fullPage:true});
});
