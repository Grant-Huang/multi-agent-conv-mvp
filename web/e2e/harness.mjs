// Only inference is replaced; the application, API, SQLite and file tools are real.
import http from 'node:http';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { createWriteStream } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('../../',import.meta.url));
const evidence=path.join(root,'web/test-results/runtime');
await mkdir(evidence,{recursive:true});
const dir=await mkdtemp(path.join(tmpdir(),'agentnexus-verify-'));
const workspace=path.join(dir,'files');
await mkdir(workspace,{recursive:true});
const log=createWriteStream(path.join(evidence,'backend.log'));
const modelLog=createWriteStream(path.join(evidence,'model.jsonl'));
const model=http.createServer(async(req,res)=>{
  if(req.url==='/v1/models'){res.setHeader('content-type','application/json');res.end(JSON.stringify({data:[{id:'verification-model'}]}));return;}
  if(req.url!=='/v1/chat/completions'){res.writeHead(404);res.end();return;}
  try {
    let text='';for await(const chunk of req)text+=chunk;
    const data=JSON.parse(text), messages=data.messages;
    const schema=data.response_format?.json_schema?.name;
    const last=[...messages].reverse().find(m=>m.role==='user')?.content??'';
    const goal=last.split('\n')[0];
    const system=messages[0]?.content??'';
    modelLog.write(JSON.stringify({schema:schema??'text',at:Date.now()})+'\n');
    let result;
    if(schema==='turn_decision') {
      const delegate=!last.startsWith('你好');
      result={action:delegate?'delegate':'respond',question:'',task_goal:delegate?last:'',task_id:'',follow_up_kind:'',change:''};
    }else if(schema==='route_decision') {
      result={task_class:'L4',execution_role:'general',required_tools:['write_file'],risk_level:goal.includes('审批')?'high':'medium',reason:'受控验证路由'};
    }else if(schema==='general_step') {
      if(messages[1]?.content.includes('慢验证任务')) {
        const timer=setTimeout(()=>res.end(),120_000);
        res.on('close',()=>{clearTimeout(timer);modelLog.write(JSON.stringify({event:'slow-disconnected'})+'\n');});
        return;
      }
      const done=messages.some(m=>m.role==='user'&&m.content.startsWith('工具 write_file 的返回结果'));
      result=done?{thought:'完成',action:'final',tool:'',args:{},answer:'验证报告已写入 reports/verification.md'}:
        {thought:'写入',action:'tool_call',tool:'write_file',args:{path:'reports/verification.md',content:'# 验证报告\n真实文件工具已写入。\n'},answer:''};
    }else if(schema==='review_verdict')result={verdict:'pass',issues:[],comment:'受控验证通过'};
    else result=system.includes('直接回答用户的最新消息')?'你好，验证成功。':'验证报告已完成，请查看产物。';
    const content=typeof result==='string'?result:JSON.stringify(result);
    res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
    for(const piece of ['<think>私有思考</think>',...content.match(/.{1,8}/gs)])res.write(`data: ${JSON.stringify({choices:[{delta:{content:piece}}]})}\n\n`);
    res.end(`data: ${JSON.stringify({choices:[{delta:{},finish_reason:'stop'}]})}\n\ndata: [DONE]\n\n`);
  }catch(e){res.writeHead(500);res.end(String(e));}
});
await new Promise(resolve=>model.listen(0,'127.0.0.1',resolve));
const modelPort=model.address().port;
const config=path.join(dir,'config.toml');
const configText=`[server]\nbind="127.0.0.1:18787"\ndb_path=${JSON.stringify(path.join(dir,'app.db'))}\nstatic_dir=${JSON.stringify(path.join(root,'web/dist'))}\n[llm]\nbase_url="http://127.0.0.1:${modelPort}/v1"\nmodel="verification-model"\nmax_retries=0\ntimeout_secs=120\n[agents.general]\nworkspace_dir=${JSON.stringify(workspace)}\nwebsearch_command=""\n[agents.coding]\nenabled=false\n[acp]\nauto_detect=false\n[nodes]\ndiscovery=false\n[logging]\ndir=${JSON.stringify(path.join(dir,'logs'))}\n`;
await writeFile(config,configText);
const binary=path.join(root,'target/debug',process.platform==='win32'?'agentnexus.exe':'agentnexus');
const child=spawn(binary,[config],{cwd:root,env:{...process.env,RUST_LOG:'info'},stdio:['ignore','pipe','pipe']});
child.stdout.pipe(log,{end:false});child.stderr.pipe(log,{end:false});
await writeFile(path.join(evidence,'state.json'),JSON.stringify({workspace,directory:dir,pid:child.pid,baseline:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim()},null,2));
let closing=false;
function shutdown(code=0){
  if(closing)return;closing=true;
  model.closeAllConnections();model.close();
  child.kill('SIGTERM');
  const force=setTimeout(()=>child.kill('SIGKILL'),5000);
  force.unref();
  child.once('close',()=>{clearTimeout(force);log.end();modelLog.end();process.exitCode=code;});
  if(child.exitCode!==null){clearTimeout(force);log.end();modelLog.end();process.exitCode=code;}
}
process.on('SIGTERM',()=>shutdown());process.on('SIGINT',()=>shutdown());
child.on('error',e=>{console.error(e);shutdown(1);});
child.on('exit',code=>{if(!closing){console.error(`backend exited: ${code}`);shutdown(code||1);}});
console.log(`Verification workspace: ${dir}`);
