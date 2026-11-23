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
// WP0-2 / E2E-CODEFLOW：coding/ACP 默认 workdir 指向临时 git 仓库，便于收集 git diff 证据
const codeRepo=path.join(dir,'code');
await mkdir(codeRepo,{recursive:true});
execFileSync('git',['init'],{cwd:codeRepo});
execFileSync('git',['config','user.email','e2e@agentnexus.test'],{cwd:codeRepo});
execFileSync('git',['config','user.name','e2e'],{cwd:codeRepo});
await writeFile(path.join(codeRepo,'README.md'),'# verification code repo\n');
execFileSync('git',['add','README.md'],{cwd:codeRepo});
execFileSync('git',['commit','-m','init'],{cwd:codeRepo});
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
      // AG.4：对话里管理助手（开关在下面的配置里打开）
      const manage=(op,id,role)=>({action:'manage_agents',question:'',task_goal:'',task_id:'',follow_up_kind:'',change:'',agent_op:op,agent_id:id,agent_role:role});
      if(last.includes('接进来验证'))result=manage('connect','claude','');
      else if(last.includes('接个写代码的助手验证'))result=manage('connect','','coding');
      else {
      const delegate=!last.startsWith('你好');
      result={action:delegate?'delegate':'respond',question:'',task_goal:delegate?last:'',task_id:'',follow_up_kind:'',change:''};
      }
    }else if(schema==='route_decision') {
      // AG.3：要求改代码时路由到 coding（测试服务里没有内置 coding agent）
      if(goal.includes('改代码验证'))result={task_class:'L2',execution_role:'coding',required_tools:[],risk_level:'medium',reason:'受控验证路由',template_id:'',deliverable:'',slots_ok:'no'};
      else if(goal.includes('报价单模板'))result={task_class:'L2',execution_role:'general',required_tools:[],risk_level:'low',reason:'受控验证模板拆分',template_id:'quote',deliverable:'报价单',slots_ok:'yes'};
      else result={task_class:'L4',execution_role:'general',required_tools:['write_file'],risk_level:goal.includes('审批')?'high':'medium',reason:'受控验证路由',template_id:'',deliverable:'',slots_ok:'no'};
    }else if(schema==='general_step') {
      // 失败验证：模型服务返回 500，General 出错，任务以 FAILED 结束（用于失败卡片用例）
      if(messages[1]?.content.includes('失败验证任务')) {
        res.writeHead(500,{'content-type':'text/plain'});
        res.end('受控失败');
        return;
      }
      if(messages[1]?.content.includes('慢验证任务')) {
        const timer=setTimeout(()=>res.end(),120_000);
        res.on('close',()=>{clearTimeout(timer);modelLog.write(JSON.stringify({event:'slow-disconnected'})+'\n');});
        return;
      }
      // WP4 / E2E-TPL：模板分支直接输出信封 JSON（不走 write_file）
      const blob=messages.map(m=>m.content||'').join('\n');
      const branchMatch=blob.match(/branch_id 必须是 `([^`]+)`/)||goal.match(/· 分支 (\S+)/);
      if(branchMatch){
        const bid=branchMatch[1];
        const payloads={damage:{verdict:'可修',hours:'6'},parts:{shortage_amount:'1200'},history:{warranty_risk:'低'}};
        const payload=payloads[bid]||{ok:'1'};
        result={thought:'信封',action:'final',tool:'',args:{},answer:JSON.stringify({branch_id:bid,status:'ok',missing:'',payload})};
      }else{
      const done=messages.some(m=>m.role==='user'&&m.content.startsWith('工具 write_file 的返回结果'));
      result=done?{thought:'完成',action:'final',tool:'',args:{},answer:'验证报告已写入 reports/verification.md'}:
        {thought:'写入',action:'tool_call',tool:'write_file',args:{path:'reports/verification.md',content:'# 验证报告\n真实文件工具已写入。\n'},answer:''};
      }
    }else if(schema==='review_verdict'){
      // E2E-REV：目标含「人工验收」时进入待验收（needs_human + result）
      const blob=messages.map(m=>m.content||'').join('\n');
      result=blob.includes('人工验收')
        ?{verdict:'needs_human',issues:['需人工确认'],comment:'需要人工验收'}
        :{verdict:'pass',issues:[],comment:'受控验证通过'};
    }
    else if(system.includes('现在没有可用的执行者'))result='现在没有能写代码的助手，这件事还没有开始做。你可以先接入一个写代码的助手。';
    else result=system.includes('直接回答用户的最新消息')?'你好，验证成功。':'验证报告已完成，请查看产物。';
    const content=typeof result==='string'?result:JSON.stringify(result);
    res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
    for(const piece of ['<think>私有思考</think>',...content.match(/.{1,8}/gs)])res.write(`data: ${JSON.stringify({choices:[{delta:{content:piece}}]})}\n\n`);
    res.end(`data: ${JSON.stringify({choices:[{delta:{},finish_reason:'stop'}]})}\n\ndata: [DONE]\n\n`);
  }catch(e){res.writeHead(500);res.end(String(e));}
});
await new Promise(resolve=>model.listen(0,'127.0.0.1',resolve));
const modelPort=model.address().port;
// AG.3：假装已安装 claude-agent-acp（包装脚本指向自带的模拟 ACP agent，握手是真实的）
const bin=path.join(dir,'bin');
await mkdir(bin,{recursive:true});
const mockAcp=path.join(root,'target/debug',process.platform==='win32'?'an-mock-acp.exe':'an-mock-acp');
await writeFile(path.join(bin,'claude-agent-acp'),`#!/bin/sh\nexec ${JSON.stringify(mockAcp)} "$@"\n`,{mode:0o755});
// AG.5 第 3、4 层：一个启动就失败的助手 broken（错误输出里带客户名 ACME），模拟的 DLP 服务（经 [egress]）把 ACME 换成占位符
const broken=path.join(bin,'broken-agent');
await writeFile(broken,`#!/bin/sh\necho "启动失败：客户 ACME 的配置不对" >&2\nexit 1\n`,{mode:0o755});
const dlpFixture=path.join(root,'crates/server/tests/fixtures/dlp_server.py');
const config=path.join(dir,'config.toml');
const configText=`[server]\nbind="127.0.0.1:18787"\ndb_path=${JSON.stringify(path.join(dir,'app.db'))}\nstatic_dir=${JSON.stringify(path.join(root,'web/dist'))}\n[llm]\nbase_url="http://127.0.0.1:${modelPort}/v1"\nmodel="verification-model"\nmax_retries=0\ntimeout_secs=120\n[agents.general]\nworkspace_dir=${JSON.stringify(workspace)}\nwebsearch_command=""\n[agents.coding]\nenabled=false\nworkdir=${JSON.stringify(codeRepo)}\n[agents.reviewer]\nenabled=true\n[acp]\nauto_detect=false\n[[acp.agents]]\nid="hand"\nname="hand"\ncommand=${JSON.stringify(mockAcp)}\nrole="reviewer"\nenv={AN_MOCK_ACP_REPLY="建议检查 [CUSTOMER_001] 的配置文件"}\n[[acp.agents]]\nid="broken"\nname="broken"\ncommand=${JSON.stringify(broken)}\nrole="reviewer"\n[egress]\nmode="audit"\ndlp_command="python3"\ndlp_args=[${JSON.stringify(dlpFixture)}]\ntimeout_ms=5000\n[nodes]\ndiscovery=false\n[conversation]\nagent_management=true\n[mom]\nworld_mock=true\n[logging]\ndir=${JSON.stringify(path.join(dir,'logs'))}\n`;
await writeFile(config,configText);
const binary=path.join(root,'target/debug',process.platform==='win32'?'agentnexus.exe':'agentnexus');
const child=spawn(binary,[config],{cwd:root,env:{...process.env,RUST_LOG:'info',PATH:`${bin}${path.delimiter}${process.env.PATH??''}`},stdio:['ignore','pipe','pipe']});
child.stdout.pipe(log,{end:false});child.stderr.pipe(log,{end:false});
await writeFile(path.join(evidence,'state.json'),JSON.stringify({workspace,codeRepo,directory:dir,pid:child.pid,baseline:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim()},null,2));
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
