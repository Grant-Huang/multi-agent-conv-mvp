import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// RT-TPL-01 → rt_tpl_01_
export function runtimePrefix(id) {
  return id.toLowerCase().replaceAll('-', '_') + '_';
}

export function validateMap(features, { exists, caseIds, testIds, runtimeCaseIds = [], runtimeTestIds = [] }) {
  const errors = [], ids = new Set(), mapped = new Set(), runtimeMapped = new Set();
  if (!Array.isArray(features)) return ['features must be an array'];
  for (const f of features) {
    if (!f.id || ids.has(f.id)) errors.push(`duplicate or empty feature: ${f.id}`);
    ids.add(f.id);
    if (!f.document || !exists(f.document)) errors.push(`${f.id}: missing document ${f.document}`);
    if (!Array.isArray(f.sources) || !f.sources.length) errors.push(`${f.id}: sources required`);
    for (const source of f.sources ?? []) if (!exists(source)) errors.push(`${f.id}: missing source ${source}`);
    if (!['browser','backend','manual'].includes(f.coverage)) errors.push(`${f.id}: invalid coverage`);
    if (!Array.isArray(f.cases)) errors.push(`${f.id}: cases required`);
    for (const id of f.cases ?? []) {
      mapped.add(id);
      if (!caseIds.includes(id)) errors.push(`${f.id}: unknown case ${id}`);
      if (f.coverage === 'browser' && !testIds.includes(id)) errors.push(`${f.id}: missing browser test ${id}`);
    }
    if (f.coverage === 'browser' && !f.cases?.length) errors.push(`${f.id}: browser coverage needs cases`);
    // 运行时用例（RT-*）：必须在 CASES.md 定义，并且有对应的 Rust 测试函数（前缀 rt_xxx_NN_）
    for (const id of f.runtime_cases ?? []) {
      runtimeMapped.add(id);
      if (!runtimeCaseIds.includes(id)) errors.push(`${f.id}: unknown runtime case ${id}`);
      if (!runtimeTestIds.some(t => t.startsWith(runtimePrefix(id)))) errors.push(`${f.id}: missing runtime test for ${id}`);
    }
  }
  for (const id of testIds) if (!mapped.has(id)) errors.push(`unmapped browser test ${id}`);
  for (const t of runtimeTestIds) {
    const owner = runtimeCaseIds.find(id => t.startsWith(runtimePrefix(id)));
    if (!owner || !runtimeMapped.has(owner)) errors.push(`unmapped runtime test ${t}`);
  }
  if (new Set(testIds).size !== testIds.length) errors.push('duplicate browser case IDs');
  return errors;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const features = JSON.parse(readFileSync(path.join(root,'docs/testing/features.json'),'utf8'));
  const cases = readFileSync(path.join(root,'docs/testing/CASES.md'),'utf8');
  const tests = readdirSync(path.join(root,'web/e2e')).filter(f=>f.endsWith('.spec.ts')).map(f=>readFileSync(path.join(root,'web/e2e',f),'utf8')).join('\n');
  const caseIds = [...cases.matchAll(/^\| (E2E-[A-Z]+) \|/gm)].map(m=>m[1]);
  const testIds = [...tests.matchAll(/test\('(E2E-[A-Z]+):/g)].map(m=>m[1]);
  const runtimeCaseIds = [...cases.matchAll(/^\| (RT-[A-Z0-9]+-[0-9]+) \|/gm)].map(m=>m[1]);
  // Rust 集成测试：crates/*/tests/*.rs 中以 rt_ 开头的测试函数
  const rustTests = [];
  for (const crate of readdirSync(path.join(root,'crates'))) {
    const dir = path.join(root,'crates',crate,'tests');
    if (!existsSync(dir)) continue;
    for (const f of readdirSync(dir).filter(f=>f.endsWith('.rs'))) {
      for (const m of readFileSync(path.join(dir,f),'utf8').matchAll(/fn (rt_[a-z0-9_]+)\s*\(/g)) rustTests.push(m[1]);
    }
  }
  const errors = validateMap(features, {exists:p=>!path.isAbsolute(p) && !p.split('/').includes('..') && existsSync(path.join(root,p)),caseIds,testIds,runtimeCaseIds,runtimeTestIds:rustTests});
  if (errors.length) { console.error(errors.join('\n')); process.exitCode=1; }
  else console.log(`Feature map valid: ${features.length} features, ${testIds.length} browser cases`);
}
