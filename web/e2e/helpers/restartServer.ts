// E2E-RESTART（GS6 重启分支）专用环境：受控模型 + 可中途重启的后端（同一 db_path）。
// 与 harness.mjs 的区别：harness 一次调用只有一个服务进程；本环境在测试内 kill/重启后端，
// 验证启动恢复语义（WP7 / G08）。端口用 18793（规约：18792 起往后，不复用其他 spec）。
import http from 'node:http';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../../../', import.meta.url));

// 受控模型（口径对齐 harness.mjs）：
// - turn_decision：非「你好」一律委派；
// - route_decision：general + write_file（medium）；
// - general_step：目标含「慢验证任务」时挂起到断连（模拟执行中），否则 write_file → final；
// - review_verdict：目标含「人工验收」→ needs_human，否则 pass。
function startModel(): Promise<{ server: http.Server; port: number }> {
  const server = http.createServer(async (req, res) => {
    if (req.url === '/v1/models') {
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ data: [{ id: 'verification-model' }] }));
      return;
    }
    if (req.url !== '/v1/chat/completions') {
      res.writeHead(404);
      res.end();
      return;
    }
    try {
      let text = '';
      for await (const chunk of req) text += chunk;
      const data = JSON.parse(text) as {
        stream?: boolean;
        messages: { role: string; content: string }[];
        response_format?: { json_schema?: { name?: string } };
      };
      const schema = data.response_format?.json_schema?.name;
      const messages = data.messages ?? [];
      const last = [...messages].reverse().find((m) => m.role === 'user')?.content ?? '';
      let result: unknown;
      if (schema === 'turn_decision') {
        const delegate = !last.startsWith('你好');
        result = {
          action: delegate ? 'delegate' : 'respond',
          question: '',
          task_goal: delegate ? last : '',
          task_id: '',
          follow_up_kind: '',
          change: '',
        };
      } else if (schema === 'route_decision') {
        result = {
          task_class: 'L4',
          execution_role: 'general',
          required_tools: ['write_file'],
          risk_level: 'medium',
          reason: '受控验证路由',
          template_id: '',
          deliverable: '',
          slots_ok: 'no',
        };
      } else if (schema === 'general_step') {
        if (messages[1]?.content.includes('慢验证任务')) {
          // 挂起：客户端断开（后端重启）才结束
          res.on('close', () => {
            try {
              res.end();
            } catch {
              /* 已断开 */
            }
          });
          return;
        }
        const done = messages.some(
          (m) => m.role === 'user' && m.content.startsWith('工具 write_file 的返回结果'),
        );
        result = done
          ? {
              thought: '完成',
              action: 'final',
              tool: '',
              args: {},
              answer: '重启验证报告已写入 reports/restart.md',
            }
          : {
              thought: '写入',
              action: 'tool_call',
              tool: 'write_file',
              args: { path: 'reports/restart.md', content: '# 重启验证报告\n' },
              answer: '',
            };
      } else if (schema === 'review_verdict') {
        const blob = messages.map((m) => m.content || '').join('\n');
        result = blob.includes('人工验收')
          ? { verdict: 'needs_human', issues: ['需人工确认'], comment: '需要人工验收' }
          : { verdict: 'pass', issues: [], comment: '受控验证通过' };
      } else {
        result = '【交付】已处理。';
      }
      const content =
        typeof result === 'string' ? result : (JSON.stringify(result) as string);
      if (data.stream === true) {
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
        res.write(`data: ${JSON.stringify({ choices: [{ delta: { content } }] })}\n\n`);
        res.end(
          `data: ${JSON.stringify({ choices: [{ delta: {}, finish_reason: 'stop' }] })}\n\ndata: [DONE]\n\n`,
        );
      } else {
        res.setHeader('content-type', 'application/json');
        res.end(
          JSON.stringify({ choices: [{ message: { role: 'assistant', content } }] }),
        );
      }
    } catch (e) {
      res.writeHead(500);
      res.end(String(e));
    }
  });
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address();
      resolve({ server, port: typeof addr === 'object' && addr ? addr.port : 0 });
    });
  });
}

function waitHealth(base: string, child: ChildProcess, deadlineMs: number): Promise<void> {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const tick = async () => {
      if (child.exitCode !== null || child.signalCode !== null) {
        reject(new Error(`backend exited early: ${child.exitCode} ${child.signalCode}`));
        return;
      }
      try {
        const r = await fetch(`${base}/api/health`);
        if (r.ok) {
          resolve();
          return;
        }
      } catch {
        /* 未就绪 */
      }
      if (Date.now() - start > deadlineMs) {
        reject(new Error('backend health timeout'));
        return;
      }
      setTimeout(tick, 200);
    };
    void tick();
  });
}

export interface RestartEnv {
  base: string;
  dir: string;
  restart(): Promise<void>;
  close(): Promise<void>;
}

export async function startRestartEnv(port = 18793): Promise<RestartEnv> {
  const dir = await mkdtemp(path.join(tmpdir(), 'an-e2e-restart-'));
  const files = path.join(dir, 'files');
  await mkdir(files, { recursive: true });
  const dbPath = path.join(dir, 'an.db');
  const configPath = path.join(dir, 'config.toml');
  const binary = path.join(
    root,
    'target/debug',
    process.platform === 'win32' ? 'agentnexus.exe' : 'agentnexus',
  );
  const { server: model, port: modelPort } = await startModel();
  const configText = `[server]\nbind="127.0.0.1:${port}"\ndb_path=${JSON.stringify(dbPath)}\nstatic_dir=${JSON.stringify(path.join(root, 'web/dist'))}\n[llm]\nbase_url="http://127.0.0.1:${modelPort}/v1"\nmodel="verification-model"\nmax_retries=0\ntimeout_secs=120\n[agents.general]\nworkspace_dir=${JSON.stringify(files)}\nwebsearch_command=""\n[agents.coding]\nenabled=false\n[agents.reviewer]\nenabled=true\n[acp]\nauto_detect=false\n[nodes]\ndiscovery=false\n[mom]\nworld_mock=true\n`;
  let child: ChildProcess | null = null;

  const boot = async (): Promise<void> => {
    await writeFile(configPath, configText);
    // 端口预留→监听交接窗口（testing/README「子进程HTTP服务就绪」）：旧进程退出后的
    // TIME_WAIT 可能让新进程 bind 失败即退出。对「启动即退出」做有界重试（3 次、1s 退避），
    // 30s 健康门不变；只重试基础设施就绪，不重试业务断言。
    const base = `http://127.0.0.1:${port}`;
    let lastErr: unknown = new Error('boot not attempted');
    for (let attempt = 1; attempt <= 3; attempt++) {
      child = spawn(binary, [configPath], {
        cwd: root,
        env: { ...process.env, RUST_LOG: 'info' },
        stdio: ['ignore', 'ignore', 'pipe'],
      });
      let stderr = '';
      child.stderr?.on('data', (d: Buffer) => {
        stderr += d.toString();
        if (stderr.length > 20000) stderr = stderr.slice(-10000);
      });
      child.on('exit', (code) => {
        if (code !== null && code !== 0 && code !== 143 && code !== 15) {
          console.error(`[restartEnv] backend exited code=${code} stderr=${stderr.slice(-2000)}`);
        }
      });
      await new Promise((r) => setTimeout(r, 300));
      try {
        await waitHealth(base, child, 30_000);
        return;
      } catch (e) {
        lastErr = e;
        if (child.exitCode === null) {
          child.kill('SIGKILL');
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    }
    throw lastErr;
  };

  await boot();

  return {
    base: `http://127.0.0.1:${port}`,
    dir,
    async restart() {
      if (!child) throw new Error('backend not running');
      const old = child;
      old.kill('SIGTERM');
      const force = setTimeout(() => old.kill('SIGKILL'), 5000);
      await new Promise<void>((resolve) => {
        old.once('exit', () => {
          clearTimeout(force);
          resolve();
        });
      });
      await new Promise((r) => setTimeout(r, 200));
      await boot();
    },
    async close() {
      if (child) {
        child.kill('SIGKILL');
      }
      model.closeAllConnections();
      model.close();
    },
  };
}
