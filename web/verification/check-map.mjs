import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export function validateMap(features, { exists, caseIds, testIds }) {
  const errors = [], ids = new Set(), mapped = new Set();
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
  }
  for (const id of testIds) if (!mapped.has(id)) errors.push(`unmapped browser test ${id}`);
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
  const errors = validateMap(features, {exists:p=>!path.isAbsolute(p) && !p.split('/').includes('..') && existsSync(path.join(root,p)),caseIds,testIds});
  if (errors.length) { console.error(errors.join('\n')); process.exitCode=1; }
  else console.log(`Feature map valid: ${features.length} features, ${testIds.length} browser cases`);
}
