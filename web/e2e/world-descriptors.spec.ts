import { test, expect } from '@playwright/test';

// 连接器描述符与健康探测（契约 K4/K6，docs/contracts/connector.md §5/§6④；Issue #488）。
// 默认 harness（[mom] world_mock=true）挂载 /api/world/descriptors 与各后端 health。

test('E2E-WADESC: 描述符含 K6 最小字段，健康探测明确返回可用形态，未知后端 404', async ({
  request,
}) => {
  const res = await request.get('/api/world/descriptors');
  expect(res.ok()).toBeTruthy();
  const body = await res.json();
  const list: Array<Record<string, unknown>> = body.descriptors;
  expect(list).toHaveLength(3);
  const ids = list.map(d => d.id);
  expect(ids).toEqual(['world.mock', 'world.des', 'world.prod']);
  for (const d of list) {
    for (const field of ['id', 'kind', 'transport', 'endpoint', 'capabilities', 'health', 'owner']) {
      expect(d[field], `${d.id} 缺 K6 字段 ${field}`).toBeDefined();
    }
    expect(d.kind).toBe('business');
    expect(d.owner).toBe('line3');
    // K2：当前均不持有外部凭据；真实站点接入时必须改为引用形态
    expect(d.credentials).toBeNull();
  }
  for (const [backend, mode] of [
    ['mock', 'in_process'],
    ['des', 'in_process'],
    ['prod', 'stub'],
  ] as const) {
    const r = await request.get(`/api/world/${backend}/health`);
    expect(r.ok(), backend).toBeTruthy();
    const h = await r.json();
    expect(h.status).toBe('ok');
    expect(h.mode).toBe(mode);
    expect(h.detail).toBeTruthy();
  }
  // K4：未知后端明确 404，不得无声失败
  const miss = await request.get('/api/world/no-such/health');
  expect(miss.status()).toBe(404);
});
