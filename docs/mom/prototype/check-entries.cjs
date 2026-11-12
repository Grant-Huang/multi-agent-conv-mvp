// UI01/02/03/06/12: standalone simulated-workbench acceptance, not production auth.
const {chromium}=require('../../../web/node_modules/@playwright/test');
const assert=require('node:assert/strict');
(async()=>{
 const b=await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH||'/usr/bin/chromium',args:['--no-sandbox']});
 try{
 const p=await b.newPage({viewport:{width:1440,height:1000}});p.setDefaultTimeout(6000);const errors=[];p.on('pageerror',e=>errors.push(e.message));
 await p.goto(process.env.PROTOTYPE_URL||'http://127.0.0.1:18793/');
 assert.deepEqual(await p.locator('#nav button').allTextContents(),['▤看板','✓任务','▥资源']);
 assert.equal(await p.locator('[data-case-card]').count(),2);
 await p.locator('[data-scope="team"]').click();assert.equal(await p.locator('[data-case-card]').count(),1);
 await p.locator('#nav [data-entry="tasks"]').click();
 assert.equal(await p.locator('[data-scope="team"]').getAttribute('aria-pressed'),'true');
 assert.equal(await p.locator('[data-task="reply"]').count(),0);
 await p.locator('#nav [data-entry="board"]').click();
 assert.equal(await p.locator('[data-case-card]').count(),1);
 await p.locator('[data-scope="shop"]').click();assert.equal(await p.locator('[data-case-card]').count(),3);
 await p.locator('[data-scope="mine"]').click();
 await p.locator('[data-board-dim="机"]').click();assert.equal(await p.locator('[data-case-card]').count(),1);
 await p.evaluate(()=>scrollTo(0,150));
 const returnY=await p.evaluate(()=>scrollY);
 await p.locator('[data-open-case="A"]').first().click();
 await p.getByRole('heading',{name:'插单 A · B 线恢复与交付',exact:true}).waitFor();
 await p.locator('.worktabs [data-worktab="decision"]').click();
 await p.locator('#approve').click();await p.locator('#understand').check();await p.locator('#confirmDecision').click();
 await p.locator('#backList').click();await p.waitForFunction(y=>Math.abs(scrollY-y)<2,returnY);assert.equal(await p.locator('[data-board-dim="机"]').getAttribute('aria-pressed'),'true');
 await p.locator('[data-case-card="A"]').getByText('已批准 · 待执行',{exact:true}).waitFor();
 await p.locator('#nav [data-entry="tasks"]').click();
 assert.equal(await p.locator('[data-group="done"]').getAttribute('open'),null);
 assert.equal(await p.locator('[data-group="blocked"]').getAttribute('open'),'');
 assert.equal(await p.locator('[data-task="approval"]').count(),1);
 await p.locator('[data-task="tool"] [data-task-action]').click();
 await p.locator('#nodeDialog').getByRole('heading',{name:'刀具调拨 · 等待仓储受理'}).waitFor();
 await p.locator('#acceptTool').click();await p.keyboard.press('Escape');
 await p.locator('#backList').click();
 await p.locator('[data-task="tool"]').getByText('已受理，等待实物交接',{exact:true}).waitFor();
 await p.locator('#nav [data-entry="resources"]').click();
 await p.locator('[data-resource-mode="all"]').click();await p.locator('[data-dim="料"]').click();
 await p.locator('[data-resource="tool"]').getByText('已受理，等待实物交接',{exact:true}).waitFor();
 await p.locator('[data-resource="tool"] [data-resource-action]').click();
 assert.equal(await p.locator('#workspace').getAttribute('data-case-id'),'A');
 await p.locator('#acceptTool').waitFor();assert.equal(await p.locator('#acceptTool').isEnabled(),false);
 await p.keyboard.press('Escape');await p.locator('#backList').click();
 assert.equal(await p.locator('[data-dim="料"]').getAttribute('aria-pressed'),'true');
 await p.locator('[data-dim="全部"]').click();assert.equal(await p.locator('[data-resource-group]').count(),6);
 await p.locator('#nav [data-entry="board"]').click();await p.locator('[data-board-dim="全部"]').click();
 await p.locator('[data-open-case="A"]').first().click();await p.locator('#openChat').click();await p.locator('#prompt').fill('A事项草稿');await p.locator('#closeChat').click();await p.locator('#backList').click();
 await p.locator('[data-open-case="C"]').first().click();await p.locator('#openChat').click();assert.equal(await p.locator('#prompt').inputValue(),'');
 await p.locator('#closeChat').click();await p.locator('#backList').click();await p.locator('[data-open-case="A"]').first().click();await p.locator('#openChat').click();assert.equal(await p.locator('#prompt').inputValue(),'A事项草稿');await p.locator('#closeChat').click();
 await p.locator('.worktabs [data-worktab="handover"]').click();await p.getByText('待接班确认',{exact:true}).waitFor();assert(!(await p.locator('#workspace').textContent()).includes('C101 客诉'));
 await p.locator('#backList').click();await p.setViewportSize({width:390,height:844});
 assert.equal(await p.locator('#mobileNav button').count(),3);
 for(const entry of ['board','tasks','resources']){await p.locator(`#mobileNav [data-entry="${entry}"]`).click();assert(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),entry+' no page overflow');}
 assert.deepEqual(errors,[]);console.log('PASS: three entries, scopes, folds, shared case/state, direct resource action, return context, scoped drafts, mobile');
 }finally{await b.close()}
})().catch(e=>{console.error(e);process.exit(1)});
