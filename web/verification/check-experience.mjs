// 体验地图校验：docs/experience-map/journeys.json 里的每条旅程，引用的宪章条款、功能、
// 用例和文件都必须存在；每条约束要么有确定性用例，要么显式标为欠账（debt）。
// 没有任何旅程覆盖的体验原则（CH-U*）只作提示，不算错误。规则见 docs/experience-map/README.md。
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const JOURNEY_ID = /^UX-\d{2}$/;

export function validateJourneys(journeys, { exists, charterIds, featureIds, caseIds }) {
  const errors = [];
  if (!Array.isArray(journeys)) return ['journeys must be an array'];
  const seen = new Set();
  for (const j of journeys) {
    const id = j?.id ?? '(no id)';
    if (!JOURNEY_ID.test(id)) errors.push(`${id}: id must look like UX-01`);
    if (seen.has(id)) errors.push(`duplicate journey ${id}`);
    seen.add(id);
    if (!j.title) errors.push(`${id}: title required`);
    for (const key of ['document', 'persona']) {
      if (!j[key] || !exists(j[key])) errors.push(`${id}: missing ${key} ${j[key]}`);
    }
    if (!Array.isArray(j.charter) || !j.charter.length) errors.push(`${id}: charter refs required`);
    for (const c of j.charter ?? []) if (!charterIds.includes(c)) errors.push(`${id}: unknown charter clause ${c}`);
    for (const f of j.features ?? []) if (!featureIds.includes(f)) errors.push(`${id}: unknown feature ${f}`);
    if (!Array.isArray(j.invariants) || !j.invariants.length) errors.push(`${id}: invariants required`);
    for (const inv of j.invariants ?? []) {
      const iid = inv.id ?? `${id}:?`;
      if (inv.charter && !charterIds.includes(inv.charter)) errors.push(`${iid}: unknown charter clause ${inv.charter}`);
      if (inv.case) {
        if (!caseIds.includes(inv.case)) errors.push(`${iid}: unknown case ${inv.case}`);
      } else if (inv.debt !== true) {
        errors.push(`${iid}: needs a case or debt: true`);
      }
    }
  }
  return errors;
}

export function uncoveredClauses(journeys, clauses) {
  const covered = new Set(journeys.flatMap(j => j.charter ?? []));
  return clauses.filter(c => !covered.has(c));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const read = p => readFileSync(path.join(root, p), 'utf8');
  const journeys = JSON.parse(read('docs/experience-map/journeys.json'));
  const charterIds = [...new Set([...read('docs/PRODUCT.md').matchAll(/^\| (CH-[A-Z]\d+) \|/gm)].map(m => m[1]))];
  const featureIds = JSON.parse(read('docs/testing/features.json')).map(f => f.id);
  const cases = read('docs/testing/CASES.md');
  const tests = readdirSync(path.join(root, 'web/e2e')).filter(f => f.endsWith('.spec.ts'))
    .map(f => read(path.join('web/e2e', f))).join('\n');
  const testIds = [...tests.matchAll(/test\('(E2E-[A-Z][A-Z0-9-]*):/g)].map(m => m[1]);
  // 只认“CASES.md 已登记且有浏览器测试”的用例
  const caseIds = [...cases.matchAll(/^\| (E2E-[A-Z][A-Z0-9-]*) \|/gm)].map(m => m[1]).filter(id => testIds.includes(id));
  const exists = p => !path.isAbsolute(p) && !p.split('/').includes('..') && existsSync(path.join(root, p));
  const errors = validateJourneys(journeys, { exists, charterIds, featureIds, caseIds });
  if (errors.length) { console.error(errors.join('\n')); process.exitCode = 1; }
  else {
    const gaps = uncoveredClauses(journeys, charterIds.filter(c => c.startsWith('CH-U')));
    const debt = journeys.flatMap(j => j.invariants).filter(i => i.debt).length;
    console.log(`Experience map valid: ${journeys.length} journeys, ${debt} invariant(s) without case (debt)` +
      (gaps.length ? `; not yet covered: ${gaps.join(', ')}` : ''));
  }
}
