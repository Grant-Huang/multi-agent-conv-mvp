// 体验地图（docs/experience-map/journeys.json）校验的单元测试。
// 规则见 docs/experience-map/README.md 第 4 节。
import test from 'node:test';
import assert from 'node:assert/strict';
import { validateJourneys, uncoveredClauses } from './check-experience.mjs';

const ctx = {
  exists: p => ['docs/experience-map/personas/P-SUP-NEW.md', 'docs/experience-map/journeys/UX-01.md'].includes(p),
  charterIds: ['CH-U1', 'CH-U2', 'CH-U3', 'CH-P8'],
  featureIds: ['ai-desktop'],
  caseIds: ['E2E-UX01'],
};

const good = () => ({
  id: 'UX-01',
  title: '从桌面发现异常并开始调查',
  document: 'docs/experience-map/journeys/UX-01.md',
  persona: 'docs/experience-map/personas/P-SUP-NEW.md',
  charter: ['CH-U1', 'CH-U2'],
  features: ['ai-desktop'],
  invariants: [
    { id: 'UX-01-I1', text: '对话输入可达', charter: 'CH-U1', case: 'E2E-UX01' },
    { id: 'UX-01-I9', text: '尚未写成用例的约束', charter: 'CH-U2', debt: true },
  ],
});

test('合格的旅程不报错', () => {
  assert.deepEqual(validateJourneys([good()], ctx), []);
});

test('引用不存在的宪章条款、功能、用例和文件时报错', () => {
  const j = good();
  j.charter = ['CH-U9'];
  j.features = ['nope'];
  j.invariants[0].case = 'E2E-NOPE';
  j.persona = 'docs/experience-map/personas/X.md';
  const errors = validateJourneys([j], ctx).join('\n');
  for (const word of ['CH-U9', 'nope', 'E2E-NOPE', 'personas/X.md']) assert.match(errors, new RegExp(word.replace('.', '\\.')));
});

test('约束既没有用例也没有标欠账时报错', () => {
  const j = good();
  delete j.invariants[1].debt;
  assert.match(validateJourneys([j], ctx).join('\n'), /UX-01-I9.*case.*debt/);
});

test('编号格式错误或重复时报错', () => {
  const a = good(), b = good();
  b.id = 'UX-1';
  const errors = validateJourneys([a, a, b], ctx).join('\n');
  assert.match(errors, /duplicate.*UX-01/);
  assert.match(errors, /UX-1\b/);
});

test('列出没有任何旅程覆盖的体验原则（只提示，不算错误）', () => {
  assert.deepEqual(uncoveredClauses([good()], ['CH-U1', 'CH-U2', 'CH-U3']), ['CH-U3']);
});
