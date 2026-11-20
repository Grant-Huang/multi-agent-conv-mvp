/**
 * World Adapter M0 浏览器套件共用 Mock 服务（独立端口 + world_mock）。
 * 供 DES-UI / MOM 工作台用例隔离运行，避免与默认 harness（18787）串扰。
 */
import { expect } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export const WA_MOCK_PORT = 18792;
export const WA_MOCK_BASE = `http://127.0.0.1:${WA_MOCK_PORT}`;

const root = fileURLToPath(new URL('../../../', import.meta.url));

export type WaMockHandle = {
  base: string;
  stop: () => Promise<void>;
  logs: () => string;
};

let sharedDist = '';
let sharedWork = '';

/** 一次构建 binary + 前端 dist，供本文件内多用例复用。 */
export function prepareWaMockArtifacts(): void {
  execFileSync('cargo', ['build', '--locked', '--bin', 'agentnexus'], {
    cwd: root,
    env: { ...process.env, CARGO_TARGET_DIR: 'target' },
    stdio: 'pipe',
  });
  sharedWork = mkdtempSync(path.join(tmpdir(), 'wa-browser-'));
  sharedDist = path.join(sharedWork, 'dist');
  execFileSync('npx', ['vite', 'build', '--outDir', sharedDist, '--emptyOutDir'], {
    cwd: path.join(root, 'web'),
    stdio: 'pipe',
  });
}

export async function startWaMockServer(name: string): Promise<WaMockHandle> {
  if (!sharedDist || !sharedWork) {
    throw new Error('call prepareWaMockArtifacts() in beforeAll first');
  }
  const dir = mkdtempSync(path.join(sharedWork, `${name}-`));
  const workspace = path.join(dir, 'workspace');
  mkdirSync(workspace, { recursive: true });
  const config = path.join(dir, 'config.toml');
  writeFileSync(
    config,
    [
      '[server]',
      `bind="127.0.0.1:${WA_MOCK_PORT}"`,
      `db_path=${JSON.stringify(path.join(dir, 'app.db'))}`,
      `static_dir=${JSON.stringify(sharedDist)}`,
      '[llm]',
      'base_url="http://127.0.0.1:1/v1"',
      'model="unused"',
      'max_retries=0',
      'timeout_secs=2',
      '[agents.general]',
      `workspace_dir=${JSON.stringify(workspace)}`,
      'websearch_command=""',
      '[agents.coding]',
      'enabled=false',
      '[acp]',
      'auto_detect=false',
      '[nodes]',
      'discovery=false',
      '[mom]',
      'world_mock=true',
      '[logging]',
      `dir=${JSON.stringify(path.join(dir, 'logs'))}`,
    ].join('\n'),
  );

  let logs = '';
  const child: ChildProcess = spawn(
    path.join(root, 'target/debug/agentnexus'),
    [config],
    { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  child.stdout?.on('data', (c) => {
    logs += c.toString();
  });
  child.stderr?.on('data', (c) => {
    logs += c.toString();
  });

  await expect
    .poll(
      async () => {
        if (child.exitCode !== null) throw new Error(logs);
        try {
          return (await fetch(WA_MOCK_BASE + '/api/health')).status;
        } catch {
          return 0;
        }
      },
      { timeout: 30_000 },
    )
    .toBe(200);

  return {
    base: WA_MOCK_BASE,
    logs: () => logs,
    stop: async () => {
      if (child.exitCode === null) {
        await new Promise<void>((resolve) => {
          const t = setTimeout(() => child.kill('SIGKILL'), 7000);
          child.once('exit', () => {
            clearTimeout(t);
            resolve();
          });
          child.kill('SIGINT');
        });
      }
      const out = path.join(root, 'web/test-results/wa-m0');
      mkdirSync(out, { recursive: true });
      writeFileSync(path.join(out, `${name}.log`), logs);
    },
  };
}
