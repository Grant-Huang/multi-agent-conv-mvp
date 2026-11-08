// Standalone design acceptance checks. No production API or model involved.
const { chromium } = require('../../../web/node_modules/@playwright/test');
const assert = require('node:assert/strict');
(async () => {
 const browser = await chromium.launch({executablePath:'/usr/bin/chromium',args:['--no-sandbox']});
 try {
 const page = await browser.newPage({viewport:{width:1440,height:1000}});
 page.setDefaultTimeout(6000);
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(process.env.PROTOTYPE_URL || 'http://127.0.0.1:18793/');
 await page.locator('[data-open-case="A"]').first().click();
 await page.locator('.worktabs [data-worktab="progress"]').click();
 await page.getByRole('button',{name:'资源可行性检查',exact:true}).click();
 await page.getByRole('button',{name:'询问 AI',exact:true}).click();
 assert.equal(await page.locator('#dock').isVisible(),false,'only one AI input when chat is open');
 await page.locator('#prompt').fill('为什么还在等待？');
 await page.locator('#send').click();
 await page.locator('#messages').getByText('尚缺刀具调拨回执和首件检验确认',{exact:false}).waitFor();
 await page.locator('#closeChat').click();
 await page.getByRole('button',{name:'资源',exact:true}).click();
 for(const name of ['人','机','料','法','环','测'])await page.getByRole('button',{name,exact:true}).click();
 await page.locator('[data-dim="机"]').click();
 await page.locator('[data-resource="xx"] [data-resource-action]').click();
 await page.getByRole('button',{name:'批准方案 B · 演示',exact:true}).click();
 assert.equal(await page.locator('#confirmDecision').isEnabled(),false);
 await page.locator('#understand').check();await page.locator('#confirmDecision').click();
 await page.getByText('已批准 · 待执行',{exact:true}).first().waitFor();
 await page.screenshot({path:'/tmp/mom-hifi-decision.png',fullPage:true});
 await page.getByRole('button',{name:'看板',exact:true}).click();
 await page.screenshot({path:'/tmp/mom-hifi-desktop.png',fullPage:true});
 await page.getByRole('button',{name:'看板',exact:true}).click();
 await page.locator('[data-open-case="C"]').first().click();
 await page.getByRole('heading',{name:'客户投诉 C101 · 风险遏制与调查',exact:true}).waitFor();
 await page.locator('#openChat').click();
 assert.equal(await page.locator('#messages').textContent().then(s=>s.includes('尚缺刀具')),false,'chat must be scoped to case');
 await page.locator('#closeChat').click();
 await page.locator('.worktabs [data-worktab="handover"]').click();
 await page.getByText('待接班确认',{exact:true}).waitFor();
 await page.setViewportSize({width:820,height:1180});
 await page.locator('#openChat').click();
 assert.equal(await page.locator('#chat').isVisible(),true);
 await page.screenshot({path:'/tmp/mom-hifi-pad.png',fullPage:true});
 await page.locator('#closeChat').click();
 await page.setViewportSize({width:390,height:844});
 assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page horizontal overflow');
 assert.deepEqual(errors,[]);
 console.log('PASS: DAG context, scoped AI, six dimensions, explicit approval, board, handover, Pad and mobile layout');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
