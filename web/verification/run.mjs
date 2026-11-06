import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const web=fileURLToPath(new URL('../',import.meta.url));
const root=fileURLToPath(new URL('../../',import.meta.url));
const steps=[
 ['npm',['run','test:map'],web],
 ['npm',['run','build'],web],
 ['cargo',['build','--locked','--bin','agentnexus'],root],
 ['npm',['run','test:e2e'],web],
];
for(const [cmd,args,cwd] of steps){
 const code=await new Promise((resolve,reject)=>{
   let interrupted=false;
   const grouped=process.platform!=='win32';
   const p=spawn(cmd,args,{cwd,detached:grouped,stdio:'inherit',env:{...process.env,CARGO_TARGET_DIR:'target'}});
   const forward=signal=>{interrupted=true;try {if(grouped)process.kill(-p.pid,signal);else p.kill(signal);}catch(e){if(e.code!=='ESRCH')throw e;}};
   const interrupt=()=>forward('SIGINT'), terminate=()=>forward('SIGTERM');
   process.once('SIGINT',interrupt);process.once('SIGTERM',terminate);
   p.once('error',reject);
   p.once('exit',(code,signal)=>{process.removeListener('SIGINT',interrupt);process.removeListener('SIGTERM',terminate);resolve(interrupted?130:signal?1:code??1);});
 });
 if(code!==0){process.exitCode=code;break;}
}
