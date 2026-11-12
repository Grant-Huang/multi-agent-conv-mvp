// Rebuild review screenshots from the standalone prototype, never production.
const {chromium}=require('../../../web/node_modules/@playwright/test');
const path=require('node:path');
(async()=>{const b=await chromium.launch({executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH||'/usr/bin/chromium',args:['--no-sandbox']});try{
const p=await b.newPage({viewport:{width:1440,height:1000}});await p.goto(process.env.PROTOTYPE_URL||'http://127.0.0.1:18793/');
const shot=n=>p.screenshot({path:path.join(__dirname,n),fullPage:false});
await shot('preview-desktop.png');
await p.locator('#nav [data-entry="tasks"]').click();await shot('preview-tasks.png');
await p.locator('#nav [data-entry="resources"]').click();await shot('preview-resources.png');
await p.locator('#nav [data-entry="board"]').click();await p.locator('[data-open-case="A"]').first().click();await p.locator('.worktabs [data-worktab="progress"]').click();
await p.getByRole('button',{name:'维护初筛',exact:true}).click();await shot('preview-node-result.png');await p.locator('#closeNode').click();
await p.getByRole('button',{name:'资源可行性检查',exact:true}).click();await shot('preview-node-conditions.png');await p.locator('#askNode').click();
await p.locator('#prompt').fill('为什么选择这个流程？');await p.locator('#send').click();await p.evaluate(()=>scrollTo(0,0));await shot('preview-dag-ai.png');
await p.locator('#closeChat').click();await p.locator('[data-source]').click();await shot('preview-provenance.png');await p.locator('#closeSource').click();
await p.setViewportSize({width:820,height:1180});await p.locator('#openChat').click();await shot('preview-pad.png');await p.locator('#closeChat').click();
await p.locator('#backList').click();await p.setViewportSize({width:390,height:844});await shot('preview-mobile.png');
console.log('Saved three entries, node, DAG/AI, provenance, Pad and mobile previews');
}finally{await b.close()}})().catch(e=>{console.error(e);process.exit(1)});
