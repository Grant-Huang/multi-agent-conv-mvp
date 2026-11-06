import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateMap } from './check-map.mjs';
const feature = { id: 'chat', document: 'chat.md', sources: ['chat.ts'], cases: ['E2E-CHAT'], coverage: 'browser' };
const files = new Set(['chat.md','chat.ts']);
const check = list => validateMap(list, { exists: p => files.has(p), caseIds: ['E2E-CHAT'], testIds: ['E2E-CHAT'] });
test('valid feature resolves', () => assert.deepEqual(check([feature]), []));
test('duplicate features are rejected', () => assert.ok(check([feature,feature]).some(x => x.includes('duplicate'))));
test('missing documents and sources are rejected', () => assert.ok(check([{...feature,document:'missing.md',sources:['missing.ts']}]).length >= 2));
test('unknown cases are rejected', () => assert.ok(check([{...feature,cases:['E2E-UNKNOWN']}]).length));
test('browser case must have executable test', () => assert.ok(validateMap([feature], {exists:()=>true,caseIds:['E2E-CHAT'],testIds:[]}).length));
test('unmapped browser tests are rejected', () => assert.ok(validateMap([], {exists:()=>true,caseIds:['E2E-CHAT'],testIds:['E2E-CHAT']}).length));

// ── 运行时用例（RT-*）：每条用例必须有对应的 Rust 测试函数，且映射不能遗漏 ──
const rtFeature = { id: 'templates', document: 'docs/DESIGN.md', sources: ['chat.ts'], cases: [], runtime_cases: ['RT-TPL-01'], coverage: 'backend' };
const rtOpts = { exists: () => true, caseIds: [], testIds: [], runtimeCaseIds: ['RT-TPL-01'], runtimeTestIds: ['rt_tpl_01_quote_template_loads'] };
test('runtime case with a matching Rust test resolves', () => assert.deepEqual(validateMap([rtFeature], rtOpts), []));
test('unknown runtime case is rejected', () => assert.ok(validateMap([{ ...rtFeature, runtime_cases: ['RT-TPL-99'] }], rtOpts).some(x => x.includes('unknown runtime case'))));
test('runtime case without a Rust test is rejected', () => assert.ok(validateMap([rtFeature], { ...rtOpts, runtimeTestIds: [] }).some(x => x.includes('missing runtime test'))));
test('unmapped runtime test is rejected', () => assert.ok(validateMap([], { ...rtOpts, runtimeTestIds: ['rt_tpl_01_quote_template_loads'] }).some(x => x.includes('unmapped runtime test'))));
