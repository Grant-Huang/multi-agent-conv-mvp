// Standalone design simulator: no API calls, identity integration, or persistence.
const $ = s => document.querySelector(s);
const state = {
  entry: 'board', scope: 'mine', case: 'A', work: null, chat: false,
  approved: false, toolAccepted: false, node: null,
  boardDim: '全部', dimension: '全部', resourceMode: 'gaps',
  folds: { todo: true, doing: true, blocked: true, done: false },
  resourceFolds: {}, positions: {}, origin: null,
  hist: { A: [], C: [], D: [] }, draft: { A: '', C: '', D: '' },
};
const entries = [['board', '▤', '看板'], ['tasks', '✓', '任务'], ['resources', '▥', '资源']];
const scopes = [['mine', '与我相关'], ['team', '本班组'], ['shop', '本车间']];
const cases = {
  A: { id: 'CASE-A', name: '插单 A · B 线恢复与交付', team: '机加班组', mine: true,
    owner: '张明 · 调度主管', dims: ['人', '机', '料', '法', '环', '测'], priority: '高',
    goal: '500 件合格零件，今日 16:00 前完成交付。',
    fact: 'XX 振动异常，B 线 CPK 降至 1.10。轴承磨损为待验证原因。',
    next: '确认停机与改排方案；补齐刀具、工艺和首件验证条件。', update: '09:10 · 维修组反馈现场检查',
    acceptance: '合格数量与交付记录经确认；备用机首件通过；剩余维修风险已明确接责。' },
  C: { id: 'CASE-C101', name: '客户投诉 C101 · 风险遏制与调查', team: '质量班组', mine: true,
    owner: '李宁 · 质量负责人', dims: ['料', '法', '测'], priority: '高',
    goal: '控制批次风险，按承诺回复客户，并验证纠正措施有效性。',
    fact: '疑似批次待追溯；冻结申请尚无仓库回执。根因未确认。',
    next: '追溯并确认冻结范围，初步回复与根因调查并行推进。', update: '12:00 · 销售提交初步回复',
    acceptance: '冻结范围有回执、纠正措施有效性已验证、客户承诺按约完成；24 小时报告不等于关闭。' },
  D: { id: 'CASE-PK', name: '包装区 · 配送延迟跟进', team: '包装班组', mine: false,
    owner: '王敏 · 包装班组长', dims: ['人', '料'], priority: '中',
    goal: '恢复本班包装物供应，核实对出货的实际影响。', fact: '纸箱配送延迟，班组正在核对剩余数量。',
    next: '仓储反馈配送时间，班组核验到货。', update: '10:00 · 仓储反馈',
    acceptance: '实物签收并确认包装恢复；未兑现承诺已重新安排。' },
};
const scope = () => cases[state.case].name;
const status = () => state.approved ? '已批准 · 待执行' : '待确认方案';
const footer = '<div class="footer">全部为模拟记录与脚本回复 · 不连接企业系统或模型 · 无权限集成 · 刷新恢复初始状态</div>';
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const tag = (s, tone = '') => `<span class="tag ${tone}">${s}</span>`;
const header = (k, title, sub, action = '') => `<div class="intro"><div class="eyebrow">${k}</div><div class="row wrap"><h1>${title}</h1>${action}</div><p class="muted small">${sub}</p></div>`;
const caseButton = (id, text = '查看事项', tab = 'overview') => `<button class="btn primary" data-open-case="${id}" data-open-tab="${tab}">${text} →</button>`;
function visibleCases() {
  return Object.keys(cases).filter(id => state.scope === 'shop' || (state.scope === 'mine' ? cases[id].mine : cases[id].team === '机加班组'));
}
function scopeBar() {
  return `<section class="scopebar" aria-label="查看范围"><div><span class="small muted">查看范围</span><div class="segmented">${scopes.map(([id, label]) => `<button data-scope="${id}" aria-pressed="${state.scope === id}" class="${state.scope === id ? 'active' : ''}">${label}</button>`).join('')}</div></div><span class="small muted">${state.scope === 'mine' ? '我的责任、待决定与关注事项' : state.scope === 'team' ? '机加班组 · 本班工作' : '车间内可见的全部样例'}<br><span class="small">范围筛选演示，不代表权限已接入</span></span></section>`;
}
function filterDims(attr, selected) {
  return `<div class="filterchips" aria-label="按生产维度筛选">${['全部', '人', '机', '料', '法', '环', '测'].map(x => `<button ${attr}="${x}" class="${selected === x ? 'active' : ''}" aria-pressed="${selected === x}">${x}</button>`).join('')}</div>`;
}
function stats(items) {
  return `<div class="stats">${items.map(([label, value, desc], i) => `<div class="stat ${i === 0 ? 'focus' : ''}"><div class="small">${label}</div><div class="num">${value}</div><small>${desc}</small></div>`).join('')}</div>`;
}
function board() {
  const all = visibleCases(), filtered = all.filter(id => state.boardDim === '全部' || cases[id].dims.includes(state.boardDim));
  const needs = all.includes('A') && !state.approved ? 1 : 0;
  return header('日班 · 运营态势', '看板', '先看重要事项与变化，进入事项即可查看进展或采取行动。') +
    stats([['待决定', needs, '停机与改排 · 需明确批准'], ['跟进事项', all.length, '范围内的业务问题'], ['资源缺口', resourcesInScope().filter(r => r.gap).length, '含待确认、过期与受限'], ['下一次反馈', all.includes('A') ? '09:10' : '12:00', '承诺未兑现时继续跟进']]) +
    `<div class="row sectiontitle wrap"><h2>关键事项 <span class="small muted">${filtered.length} 项</span></h2>${filterDims('data-board-dim', state.boardDim)}</div><div class="split"><section>${filtered.map(caseCard).join('') || '<div class="card muted">当前维度没有相关事项，请切换筛选。</div>'}</section><aside><section class="card"><h2>本班通知</h2><span class="tag" style="margin-top:14px">仅通知 · 不计入任务</span><h3 style="margin-top:12px">设备切换后保留首件核验记录</h3><p class="small muted">质量组发布 · 适用于本班 / 已读不等于检验通过。</p></section><section class="card"><h2>承诺与变化</h2>${all.map(id => `<div class="taskrow"><div class="taskicon">◷</div><div><h3>${cases[id].update}</h3><p class="small muted">${cases[id].name}</p>${id === 'A' ? `<p class="small">${state.approved ? '方案 v3 已批准；执行和核验仍待回执。' : '方案 v3 等待主管确认。'}</p>` : ''}<button class="textbtn" data-open-case="${id}" data-open-tab="handover">查看承诺与交接 →</button></div></div>`).join('')}</section></aside></div>`;
}
function caseCard(id) {
  const c = cases[id];
  return `<article class="card ${id === 'A' ? 'accent' : 'quality'}" data-case-card="${id}"><div class="row"><span>${tag(id === 'A' ? status() : id === 'C' ? '风险遏制中' : '配送跟进中', 'amber')}</span><span class="mono small muted">${c.id}</span></div><h2 class="cardtitle">${c.name}</h2><p class="small">${c.goal}</p><p class="small muted">${c.fact}</p><div class="facts"><div><span>牵头责任人</span>${c.owner}</div><div><span>下一次反馈</span>${c.update}</div></div><div class="cardfoot"><div class="small muted">${c.dims.map(d => tag(d)).join(' ')}<br><span>来源：业务记录模拟 / 08:36</span></div>${caseButton(id, id === 'A' && !state.approved ? '查看与处置' : '查看事项')}</div></article>`;
}
function tasksData() {
  return [
    { id: 'approval', c: 'A', group: state.approved ? 'doing' : 'todo', name: '确认停机与改排方案', owner: '张明', due: '尽快处理', detail: state.approved ? '已批准 · 待执行' : '批准方案 v3 的具体动作范围', action: 'decision' },
    { id: 'handover', c: 'A', group: 'todo', name: '核对中班交接内容', owner: '张明', due: '15:45 前', detail: '交清未完成条件、风险边界和下一次承诺', action: 'handover' },
    { id: 'tool', c: 'A', group: state.toolAccepted ? 'doing' : 'blocked', name: '调拨 φ32 铣刀至 XZ', owner: '仓储组', due: '09:20 反馈', detail: state.toolAccepted ? '已受理，等待实物交接' : '等待仓储受理；未到位不能投产', action: 'tool' },
    { id: 'inspection', c: 'A', group: 'blocked', name: '安排备用机首件检验', owner: '质量组', due: '条件满足后安排', detail: '工艺、刀具、现场试制条件尚未齐备', action: 'inspection' },
    { id: 'diagnosis', c: 'A', group: 'doing', name: '完成主轴现场检查', owner: '维修组', due: '09:10 反馈', detail: '正在核实候选原因，不得直接恢复设备', action: 'progress' },
    { id: 'maintenance', c: 'A', group: 'done', name: '维护初筛', owner: '维护诊断', due: '08:34 已完成', detail: '已提交初筛；维修及效果核验尚未完成', action: 'maintenance' },
    { id: 'quality', c: 'A', group: 'done', name: '质量遏制评估', owner: '质量评估', due: '08:34 已完成', detail: '已提交风险评估；批次复核尚未完成', action: 'quality' },
    { id: 'reply', c: 'C', group: 'todo', name: '核对客户初步回复', owner: '张明会签 / 销售提供草稿', due: '12:00 前', detail: '只承诺已确认事实与下一次反馈', action: 'decision' },
    { id: 'freeze', c: 'C', group: 'blocked', name: '确认 C101 冻结回执', owner: '仓储 / 质量组', due: '待追溯范围', detail: '申请冻结不等于已冻结', action: 'conditions' },
    { id: 'delivery', c: 'D', group: 'doing', name: '跟进纸箱配送', owner: '仓储 / 王敏', due: '10:00 反馈', detail: '预计时间未确认，待实物签收', action: 'conditions' },
  ].filter(t => visibleCases().includes(t.c));
}
function tasks() {
  const data = tasksData();
  return header('日班 · 工作推进', '任务', '按下一步工作分组；提交、受理和核验分别记录。') +
    `<div class="row sectiontitle"><h2>${state.scope === 'mine' ? '与我相关的工作' : '范围内工作'} <span class="small muted">${data.length} 项</span></h2><span class="small muted">已完成默认折叠</span></div>` +
    [['todo', state.scope === 'mine' ? '待我处理' : '待处理'], ['doing', '进行中'], ['blocked', '受阻／需协助'], ['done', '已完成']].map(([id, label]) => {
      const list = data.filter(t => t.group === id);
      return `<details class="taskgroup" data-group="${id}" ${state.folds[id] ? 'open' : ''}><summary><strong>${label}</strong><span class="tag ${id === 'blocked' ? 'amber' : ''}">${list.length} 项</span>${id === 'blocked' ? '<span class="small muted">缺失条件未满足，不会因勾选变为完成</span>' : ''}</summary><div class="groupbody">${list.map(t => `<article class="workrow" data-task="${t.id}"><div><h3>${t.name}</h3><p class="small">${t.detail}</p><p class="small muted">${cases[t.c].id} / ${cases[t.c].name} · ${t.owner} · ${t.due}</p></div><button class="btn ${id === 'todo' ? 'primary' : ''}" data-task-action="${t.id}">${id === 'done' ? '查看结果' : id === 'blocked' ? '处理条件' : '查看与处理'} →</button></article>`).join('') || '<p class="muted small">当前没有此类工作。</p>'}</div></details>`;
    }).join('');
}
const dimensionNames = { 人: '班组与资格', 机: '设备与能力', 料: '齐套与批次', 法: '工艺与版本', 环: '区域与环境', 测: '检测与放行' };
const resourceData = [
  { id: 'operators', dim: '人', refs: ['A'], name: '机加日班', status: '可投入人员待确认', detail: 'XY / XZ 技能匹配待班组长确认', gap: true, action: 'conditions' },
  { id: 'maintcrew', dim: '人', refs: ['A'], name: '维修组', status: '已接单', detail: '09:10 前反馈现场检查，不等于已修复', gap: false, action: 'progress' },
  { id: 'xx', dim: '机', refs: ['A'], name: 'CNC-XX', status: '异常受限', detail: '停机与改排须批准；未经维护核验不得恢复', gap: true, action: 'decision' },
  { id: 'xy', dim: '机', refs: ['A'], name: 'CNC-XY', status: '订单 B 占用', detail: '释放时间未确认，不得承诺可用时段', gap: true, action: 'conditions' },
  { id: 'tool', dim: '料', refs: ['A'], name: 'φ32 铣刀 / XZ', status: () => state.toolAccepted ? '已受理，等待实物交接' : '缺 1 套 · 调拨待受理', detail: '实物、规格与可用性核验前不解锁', gap: true, action: 'tool' },
  { id: 'batch', dim: '料', refs: ['C'], name: '投诉批次 C101', status: '申请冻结', detail: '等待追溯范围和 WMS 冻结回执', gap: true, action: 'conditions' },
  { id: 'box', dim: '料', refs: ['D'], name: '包装纸箱', status: '配送延迟', detail: '库存与到货时间待确认', gap: true, action: 'conditions' },
  { id: 'route', dim: '法', refs: ['A'], name: '精加工路线 v2.3', status: '版本受控 · 适用性待确认', detail: '不得将受控版本等同于适用于备用机', gap: true, action: 'conditions' },
  { id: 'traceRule', dim: '法', refs: ['C'], name: '投诉追溯规范 v1.2', status: '当前受控版本', detail: '初步回复、阶段报告与关闭分别记录', gap: false, action: 'progress' },
  { id: 'air', dim: '环', refs: ['A'], name: '精加工区压缩空气', status: '数据过期 · 未知', detail: '08:10 后无更新，不显示为正常', gap: true, action: 'conditions' },
  { id: 'temp', dim: '环', refs: ['A'], name: '区域温度', status: '22.5°C · 样例范围内', detail: '08:30 模拟读数，不能替代现场确认', gap: false, action: 'conditions' },
  { id: 'inspection', dim: '测', refs: ['A', 'C'], name: '检验组 / 检测能力', status: '首件与 C101 复检需协调', detail: '共享人员只维护一份记录，不重复预留', gap: true, action: 'inspection' },
];
const resourceStatus = r => typeof r.status === 'function' ? r.status() : r.status;
function resourcesInScope(caseId = null) {
  const ids = caseId ? [caseId] : visibleCases();
  return resourceData.filter(r => r.refs.some(id => ids.includes(id)));
}
function resourceRow(r, caseId = null) {
  const target = caseId || r.refs.find(id => visibleCases().includes(id));
  return `<article class="workrow" data-resource="${r.id}"><div><h3>${r.name} ${tag(r.gap ? '待补齐／确认' : '状态记录', r.gap ? 'amber' : 'green')}</h3><p class="small">${resourceStatus(r)}</p><p class="small muted">${r.detail}</p><span class="small muted">关联：${r.refs.filter(id => caseId ? id === caseId : visibleCases().includes(id)).map(id => cases[id].id).join(' / ')} · 来源：模拟记录</span></div><button class="btn" data-resource-action="${r.id}" data-resource-case="${target}">${r.gap ? '查看与处置' : '查看关联事项'} →</button></article>`;
}
function resources() {
  let data = resourcesInScope().filter(r => state.resourceMode === 'all' || r.gap);
  const dims = state.dimension === '全部' ? Object.keys(dimensionNames) : [state.dimension];
  return header('日班 · 条件与能力', '资源', '看当前状态，或查看完成工作还缺什么；未知与受限不显示为可用。') +
    `<div class="row wrap resourcefilters"><div class="segmented">${[['gaps', '缺口与限制'], ['all', '全部状态']].map(([id, label]) => `<button data-resource-mode="${id}" aria-pressed="${state.resourceMode === id}" class="${state.resourceMode === id ? 'active' : ''}">${label}</button>`).join('')}</div>${filterDims('data-dim', state.dimension)}</div>` +
    dims.map(dim => {
      const rows = data.filter(r => r.dim === dim), count = rows.filter(r => r.gap).length;
      const open = state.resourceFolds[dim] ?? (state.dimension !== '全部' || count > 0);
      return `<details class="resourcegroup" data-resource-group="${dim}" ${open ? 'open' : ''}><summary><b class="dimmark">${dim}</b><strong>${dimensionNames[dim]}</strong><span class="tag ${count ? 'amber' : ''}">${count ? count + ' 项缺口／限制' : rows.length ? rows.length + ' 项状态' : '当前范围无相关记录'}</span></summary><div class="groupbody">${rows.map(r => resourceRow(r)).join('') || '<p class="small muted">当前范围或筛选下无记录，不代表已验证正常。</p>'}</div></details>`;
    }).join('') + `<details class="card"><summary><strong>跨事项资源协调</strong><span class="small muted">　占用与冲突</span></summary><p class="small">${visibleCases().includes('A') ? 'XY 被订单 B 占用；释放时间未知，插单 A 尚不能承诺开工。' : '当前范围无设备排程记录。'}</p><p class="small">${visibleCases().includes('A') && visibleCases().includes('C') ? '插单 A 首件与 C101 复检争用检验组，需协调实际时段。' : '共享检验能力可能被其他工作占用，具体时段待确认。'}</p><div class="hint">资源图不承诺全局最优；安全、质量和权限先满足，实际预留未接入。</div></details>`;
}
function workspace() {
  const c = cases[state.case];
  const tabs = [['overview', '事项概览'], ['progress', '处理进展'], ['conditions', '所需条件'], ['decision', '方案与决定'], ['handover', '承诺与交接']];
  return `<section id="workspace" data-case-id="${state.case}"><div class="row wrap workspaceback"><button class="textbtn" id="backList">← 返回${entries.find(x => x[0] === state.entry)[2]}</button><span class="small muted">${scopes.find(x => x[0] === state.scope)[1]} / ${c.id}</span></div>` +
    header('事项工作区 · 模拟', c.name, `牵头：${c.owner} / ${c.team}`, tag(state.case === 'A' ? status() : '处置中', 'amber')) +
    `<div class="tabs worktabs" aria-label="事项内容">${tabs.map(([id, label]) => `<button class="tab ${state.work === id ? 'active' : ''}" data-worktab="${id}" aria-pressed="${state.work === id}">${label}</button>`).join('')}</div>` +
    ({ overview: caseOverview, progress: progress, conditions: caseConditions, decision: caseDecision, handover: handover })[state.work]() + '</section>';
}
function caseOverview() {
  const c = cases[state.case];
  return `<div class="split"><section class="card"><h2>当前情况</h2><p>${c.fact}</p><div class="facts"><div><span>业务目标</span>${c.goal}</div><div><span>下一次反馈</span>${c.update}</div></div><h3>下一步</h3><p>${c.next}</p><div class="cardfoot"><span class="small muted">${c.id} · 08:36 样例状态</span><button class="btn primary" data-worktab="${state.case === 'A' && !state.approved ? 'decision' : 'progress'}">${state.case === 'A' && !state.approved ? '确认处置方案' : '查看处理进展'} →</button></div></section><aside><section class="card"><h2>凭什么关闭？</h2><p class="small">${c.acceptance}</p><div class="hint">任务完成不自动关闭事项，需核验业务结果。</div></section></aside></div><section class="card"><div class="row"><h2>处理进展</h2><button class="textbtn" data-worktab="progress">展开处置流程 →</button></div>${state.case === 'A' ? stages() : `<p class="small">${state.case === 'C' ? '批次追溯 ∥ 风险遏制 ∥ 根因调查 → 客户回复 → 措施验证' : '确认缺口 → 配送跟进 → 实物签收 → 恢复核验'}</p>`}</section>`;
}
function progress() {
  if (state.case === 'D') return `<div class="card"><h2>配送处置进展</h2><p>确认缺口 → 配送跟进 → 实物签收 → 恢复核验</p><p class="small muted">当前等待配送时间；完整流程实例未在此原型模拟。</p><button class="btn" data-worktab="conditions">查看缺失条件</button></div>`;
  return `<div class="row sectiontitle wrap"><h2>处置流程 <span class="small muted">${state.case === 'A' ? '运行版本 v3' : '投诉遏制与调查'}</span></h2><button class="btn" data-source>流程选择依据 ↗</button></div><p class="small muted">点击已完成节点看结果，点击等待节点直接处理条件。</p>` + progressView();
}
function caseConditions() {
  return `<div class="row sectiontitle wrap"><h2>当前事项所需条件</h2>${state.case === 'A' ? '<button class="btn primary" data-action="resources">处理关键缺口</button>' : ''}</div>` + Object.keys(dimensionNames).map(dim => {
    const rows = resourcesInScope(state.case).filter(r => r.dim === dim);
    return `<details class="resourcegroup" ${rows.some(r => r.gap) ? 'open' : ''}><summary><b class="dimmark">${dim}</b><strong>${dimensionNames[dim]}</strong>${tag(rows.length ? rows.filter(r => r.gap).length + ' 项待补齐／确认' : '当前事项未关联')}</summary><div class="groupbody">${rows.map(r => resourceRow(r, state.case)).join('') || '<p class="small muted">未关联记录不等于已验证合格。</p>'}</div></details>`;
  }).join('');
}
function caseDecision() {
  if (state.case === 'A') return decision();
  return `<div class="card"><h2>${state.case === 'C' ? '初步回复与风险遏制' : '配送处置'}</h2><p>${cases[state.case].fact}</p><div class="notice">${state.case === 'C' ? '客户回复须区分已知事实与候选原因；未收到仓库回执不得宣称冻结成功。' : '到货时间未确认，不得对出货作出新承诺。'}</div><p class="small muted">本原型未模拟此事项的批准动作，不沿用插单 A 的方案或授权。</p><button class="btn" data-worktab="conditions">核对所需条件 →</button></div>`;
}
function handover() {
  const c = cases[state.case];
  return `<section class="card"><div class="row"><h2>本事项承诺与交接</h2>${tag('待接班确认', 'amber')}</div><div class="facts"><div><span>牵头／交班责任</span>${c.owner}</div><div><span>接班人</span>待指定并确认接责</div></div><h3>下一次承诺</h3><p>${c.update}</p><h3>未完成与不可越过的边界</h3><p>${state.case === 'A' ? '刀具未交接、备用机首件未核验；XX 未经维护核验不得恢复。批准方案不代表这些条件已满足。' : state.case === 'C' ? '批次范围与冻结回执待确认；根因调查未结束，阶段报告不等于投诉关闭。' : '配送与实物签收未完成；不能用预计时间代替到货证据。'}</p><h3>关闭前核对</h3><p class="small muted">${c.acceptance}</p><div class="hint">交接确认不自动关闭事项，已读也不代表接责。</div><button class="btn" style="margin-top:16px" data-ask="整理当前事项交接摘要">让 AI 整理交接摘要</button></section>`;
}
function stages() {
  return `<div class="stages">${['事件关联', '并行研判', '方案确认', '执行处置', '效果核验'].map((n, i) => `<div class="stage ${i < (state.approved ? 3 : 2) ? 'done' : i === (state.approved ? 3 : 2) ? 'active' : ''}"><b>${i < (state.approved ? 3 : 2) ? '✓' : i + 1}</b>${n}</div>`).join('')}</div>`;
}
function render() {
  $('#breadcrumb').textContent = entries.find(x => x[0] === state.entry)[2] + (state.work ? ' / 事项' : '');
  $('#nav').innerHTML = entries.map(([id, icon, label]) => `<button class="navbtn ${state.entry === id ? 'active' : ''}" data-entry="${id}" aria-label="${label}" aria-current="${state.entry === id ? 'page' : 'false'}"><i aria-hidden="true">${icon}</i><span>${label}</span></button>`).join('');
  $('#mobileNav').innerHTML = entries.map(([id, icon, label]) => `<button data-entry="${id}" aria-label="移动端${label}" aria-current="${state.entry === id ? 'page' : 'false'}">${icon}<br>${label}</button>`).join('');
  $('#main').innerHTML = (state.work ? workspace() : scopeBar() + ({ board, tasks, resources })[state.entry]()) + footer;
  $('#dockScope').textContent = state.work ? scope() : `${scopes.find(x => x[0] === state.scope)[1]} · ${entries.find(x => x[0] === state.entry)[2]} / 先选事项再讨论`;
  $('#dockAction').textContent = state.work ? '查看所需条件 ↗' : '先选择一个事项';
  $('#dockAction').disabled = !state.work;
  $('#dockAction').onclick = () => setWorkTab('conditions');
  bind();
}
const positionKey = () => state.entry + ':' + state.scope;
function savePosition() { if (!state.work) state.positions[positionKey()] = window.scrollY; }
function restorePosition(y) { requestAnimationFrame(() => window.scrollTo(0, y || 0)); }
function rememberDraft() { state.draft[state.case] = $('#prompt').value; }
function changeCase(id) {
  if (state.case !== id) { rememberDraft(); state.case = id; state.node = null; $('#prompt').value = state.draft[id]; renderMessages(); }
}
function openCase(id, tab = 'overview') {
  if (!visibleCases().includes(id)) return;
  if (!state.work) { savePosition(); state.origin = { key: positionKey(), y: window.scrollY }; }
  document.querySelectorAll('dialog[open]').forEach(d => d.close());
  changeCase(id); state.node = null; state.work = tab; render(); restorePosition(0); updateChatScope();
}
function backList() {
  if (state.chat) closeChat();
  rememberDraft(); state.work = null; state.node = null; render(); restorePosition(state.origin?.y ?? state.positions[positionKey()]);
}
function setWorkTab(tab) { if (!state.work) return; state.work = tab; state.node = null; render(); restorePosition(0); updateChatScope(); }
function changeEntry(entry) {
  savePosition(); if (state.chat) closeChat(); rememberDraft(); state.entry = entry; state.work = null; state.node = null;
  render(); restorePosition(state.positions[positionKey()]);
}
function changeScope(id) {
  savePosition(); state.scope = id; state.work = null; state.node = null;
  if (!visibleCases().includes(state.case)) changeCase(visibleCases()[0]);
  render(); restorePosition(state.positions[positionKey()]);
}
function runAction(caseId, action) {
  const tabs = { tool: 'conditions', inspection: 'conditions', resources: 'conditions', maintenance: 'progress', quality: 'progress' };
  openCase(caseId, tabs[action] || action);
  if (caseId !== 'A') return;
  if (action === 'tool' || action === 'inspection') { state.node = 'resources'; $('#nodeDialog').showModal(); conditionTask(action); }
  else if (['resources', 'maintenance', 'quality', 'execution'].includes(action)) showNode(action);
}
function bind() {
  document.querySelectorAll('[data-entry]').forEach(b => b.onclick = () => changeEntry(b.dataset.entry));
  document.querySelectorAll('[data-scope]').forEach(b => b.onclick = () => changeScope(b.dataset.scope));
  document.querySelectorAll('[data-open-case]').forEach(b => b.onclick = () => openCase(b.dataset.openCase, b.dataset.openTab));
  document.querySelectorAll('[data-worktab]').forEach(b => b.onclick = () => setWorkTab(b.dataset.worktab));
  document.querySelectorAll('[data-board-dim]').forEach(b => b.onclick = () => { state.boardDim = b.dataset.boardDim; render(); });
  document.querySelectorAll('[data-dim]').forEach(b => b.onclick = () => { state.dimension = b.dataset.dim; render(); });
  document.querySelectorAll('[data-resource-mode]').forEach(b => b.onclick = () => { state.resourceMode = b.dataset.resourceMode; render(); });
  document.querySelectorAll('[data-group]').forEach(d => d.ontoggle = () => { if (d.isConnected) state.folds[d.dataset.group] = d.open; });
  document.querySelectorAll('[data-resource-group]').forEach(d => d.ontoggle = () => { if (d.isConnected) state.resourceFolds[d.dataset.resourceGroup] = d.open; });
  document.querySelectorAll('[data-task-action]').forEach(b => b.onclick = () => { const t = tasksData().find(x => x.id === b.dataset.taskAction); runAction(t.c, t.action); });
  document.querySelectorAll('[data-resource-action]').forEach(b => b.onclick = () => { const r = resourceData.find(x => x.id === b.dataset.resourceAction); runAction(b.dataset.resourceCase, r.action); });
  document.querySelectorAll('[data-node]').forEach(b => b.onclick = () => showNode(b.dataset.node));
  document.querySelectorAll('[data-action]').forEach(b => b.onclick = () => showNode(b.dataset.action));
  document.querySelectorAll('[data-source]').forEach(b => b.onclick = showSource);
  document.querySelectorAll('[data-ask]').forEach(b => b.onclick = () => { if (!state.work) return toast('先选择事项，以明确讨论范围。'); openChat(); $('#prompt').value = b.dataset.ask; send(); });
  if ($('#backList')) $('#backList').onclick = backList;
  if ($('#approve')) $('#approve').onclick = () => { $('#understand').checked = false; $('#confirmDecision').disabled = true; $('#decisionDialog').showModal(); };
}
function showNode(key) {
  if (!state.work || state.case !== 'A') return;
  state.node = key;
  if (key === 'approval') { setWorkTab('decision'); return; }
  if (!$('#nodeDialog').open) $('#nodeDialog').showModal();
  nodePanel(key); updateChatScope();
}
const nodeData={maintenance:['维护初筛','已完成 · 08:34','维护诊断工具 · 任务 M-01','发现主轴振动异常。轴承磨损为候选原因，尚需现场检查验证。'],quality:['质量遏制评估','已完成 · 08:34','质量关联工具 · 任务 Q-01','CPK 低于控制要求。关联批次需复核；未授权解除任何质量冻结。'],resources:['资源可行性检查','等待条件 · 不可投产','排产仿真工具 · 任务 S-01','尚缺刀具调拨回执和首件检验确认；方案可供审批，但批量生产不可解锁。'],approval:['停机与改排批准', '等待主管','张明 · 人类决策节点','批准范围限定为方案 v3。安全收尾、刀具齐套与首件核验仍是执行关卡。'],execution:['执行与现场核验','等待上游','现场班组 / 系统工具','未取得对应动作的受理与执行回执，不标记为完成。']};
function node(key,x,y,cls){const d=[...nodeData[key]];if(key==='approval'&&state.approved)d[1]='已批准 · 待执行';return `<button class="node ${cls}" data-node="${key}" style="left:${x}px;top:${y}px" aria-label="${d[0]}"><span class="tag ${cls==='complete'?'green':cls==='current'?'amber':''}">${d[1].split(' · ')[0]}</span><strong>${d[0]}</strong><small>${d[2].split(' · ')[0]}</small><small class="nodeaction">${key==='approval'?'查看方案 →':cls==='complete'?'查看结果 →':key==='resources'?'处理缺失条件 →':'查看阻塞 →'}</small></button>`}

function progressView(){const c=state.case==='C';return `${c?complaintFlow():stages()+`<div class="row sectiontitle"><h2>流程依赖图 <span class="small muted">DAG v3</span></h2><span class="small muted">只读运行图 · 点击节点查看</span></div><div class="flowboard"><div class="graph"><svg class="edges" viewBox="0 0 960 345" aria-hidden="true"><defs><marker id="arrow" markerWidth="7" markerHeight="7" refX="6" refY="3" orient="auto"><path d="M0,0 L6,3 L0,6" style="fill:#9eb5af;stroke:none"/></marker></defs><path d="M174 73 H205 V172 H250"/><path d="M174 266 H205 V172 H250"/><path d="M424 172 H490"/><path class="future" d="M664 172 H750"/><path class="future" d="M424 172 H450 V52 H490"/></svg>${node('maintenance',0,25,'complete')}${node('quality',0,218,'complete')}<div class="gateway" style="left:190px;top:126px">AND<br>两项必需</div>${node('resources',250,120,'current')}<div class="small muted" style="position:absolute;left:439px;top:189px;font-size:9px">方案可评审</div>${node('approval',490,120,state.approved?'complete':'current')}${node('execution',750,120,'future')}<div class="small muted" style="position:absolute;left:0;top:0">并行研判</div><div class="gateway" style="left:490px;top:28px;background:#f3f4f3;color:#8c9691">A · 继续赶工 / 不满足质量边界</div><div class="small muted" style="position:absolute;left:490px;top:89px">${state.approved?"已选 B · A 不执行":"B 待批准 · A 不可执行"}</div></div><div class="legend"><span>● 已完成</span><span style="color:#a87629">● 等待条件 / 决策</span><span>┄ 等待上游</span><span>AND 全部必需结果满足后汇合</span></div></div><div class="hint" style="margin-top:15px">独立调查：根因核实进行中 · 不阻断保守风险遏制；调查结论不直接授权停机或解除冻结。</div>`}`;}

function complaintFlow(){return `<div class="card"><h2>先遏制，后验证；根因调查并行</h2><div class="stages"><div class="stage done">✓ 投诉登记</div><div class="stage active">02 批次追溯 ∥ 风险遏制</div><div class="stage">03 初步回复</div><div class="stage">04 阶段报告</div></div><div class="decisiongrid"><div class="hint"><h3>并行 A · 追溯与冻结确认</h3><p>MES 追溯 → WMS 冻结回执 → 确认风险边界</p><span class="tag amber">等待仓库受理</span></div><div class="hint"><h3>并行 B · 专项根因调查</h3><p>专项调查 / 汇总班次、设备、材料与检验记录</p><span class="tag">调查中 · 不阻断初步回复</span></div></div><div class="notice">初步回复只承诺已确认的事实与下一次反馈时间；未经验证的原因不得写成正式根因。</div></div>`}

function decision(){return `<div class="row sectiontitle"><h2>停机与改排 · 方案评审</h2><span class="tag">方案 v3</span></div><div class="notice">根因尚未最终确认。当前建议基于风险遏制，不依赖“轴承磨损已证实”的假设。</div><div class="decisiongrid"><section class="option danger"><div class="row"><span class="eyebrow" style="color:#8c9a96">OPTION A</span><span class="tag red">不满足质量边界</span></div><h2 style="margin:15px 0">继续使用 XX 赶工</h2><p class="small">持续加工以争取交期，但质量风险无法排除。</p><div class="metricline"><span>交期</span><span>可能满足 · 不确定</span></div><div class="metricline"><span>质量与安全</span><span>未满足，不可批准</span></div><div class="metricline"><span>处理结果</span><span>仅保留作比较</span></div></section><section class="option recommend"><div class="row"><span class="eyebrow">OPTION B</span><span class="tag green">AI 建议 · 有前提</span></div><h2 style="margin:15px 0">停用 XX，转移剩余产能</h2><p class="small">XY 与 XZ 分担加工；现场检查与备用机验证同步推进。</p><div class="metricline"><span>交期</span><span>需重算后确认</span></div><div class="metricline"><span>前提</span><span>刀具齐套、首件通过</span></div><div class="metricline"><span>其他订单影响</span><span>订单 B 可能延迟，待排程核验</span></div><div style="margin-top:18px">${state.approved?'<span class="tag green">已批准 · 待执行</span>':'<button class="btn primary" id="approve">批准方案 B · 演示</button>'} <button class="btn" data-ask="对比两个方案">与 AI 讨论取舍</button></div></section></div><div class="card" style="margin-top:20px"><h2>批准后仍需通过的关卡</h2><div class="audit"><div class="auditstep"><b>指令受理</b>调拨、停机与改排分别回执</div><div class="auditstep"><b>现场执行</b>工单执行不等于核验通过</div><div class="auditstep"><b>首件放行</b>未通过禁止批量生产</div><div class="auditstep"><b>交付重算</b>更新承诺前核验真实产能</div></div><p class="small muted">资源或方案版本变化，原批准须重新校验适用性。当前页面不下发任何实际指令。</p></div>`}

const sourceBase=$('#sourceDialog').innerHTML;
function showSource(){let html=sourceBase;if(state.case==='C')html=html.replaceAll('插单、设备告警与质检记录','投诉、出货与质检记录').replaceAll('CNC-XX → B 线 → 插单 A','客户 → 发货批次 → 工单 / 库存').replaceAll('覆盖 B 线','覆盖投诉批次 C101').replaceAll('复合扰动处置','客户投诉遏制与调查').replaceAll('v2.1','v1.2').replaceAll('振动与 CPK 同时异常','出货批次与在库批次关联').replaceAll('三类事件同时命中','投诉、追溯、遏制需求命中').replaceAll('含质量遏制、维护、改排','含批次追溯、冻结确认与报告').replaceAll('未覆盖交期与质量影响','未覆盖投诉追溯与客户回复');$('#sourceDialog').innerHTML=html;$('#closeSource').onclick=()=>$('#sourceDialog').close();$('#sourceDialog').showModal();}

function panelFrame(title,sub,body,tab='',child=false){
 const d=$('#nodeDialog');
 d.innerHTML=`<div class="panelhead"><div class="row"><span class="eyebrow">插单 A / ${child?'条件处置':'流程节点'} · 模拟</span><button class="btn" id="closeNode">返回流程</button></div><h2>${title}</h2><p class="small muted">${sub}</p></div>${tab}<div class="panelbody">${body}</div><div class="paneltail"><span class="small muted">只看结论与记录即可处置，无需先问 AI。</span><button class="btn subtle" id="askNode">询问 AI</button></div>`;
 $('#closeNode').onclick=()=>d.close();
 $('#askNode').onclick=()=>{d.close();openChat();};
 d.scrollTop=0;
}
function nodePanel(k,tab='result'){
 const d=nodeData[k];
 if(k==='resources'){conditions();return;}
 if(k==='execution'){
 panelFrame('执行与现场核验 · 等待上游','当前不能开始批量生产。先处理真正阻塞它的条件。',`<div class="notice">${state.approved?'方案已批准，但批准不等于现场执行完成。':'方案尚未批准。'}</div><div class="condition"><h3>停机与改排方案</h3><p class="small muted">责任人：张明 / ${state.approved?'已批准，等待动作回执':'等待批准'}</p><button class="btn" id="upstreamApproval">查看方案</button></div><div class="condition"><h3>生产条件未齐</h3><p class="small muted">刀具未交接、首件未核验；不能直接跳过。</p><button class="btn primary" id="upstreamConditions">处理缺失条件</button></div>`);
 $('#upstreamApproval').onclick=()=>{$('#nodeDialog').close();openCase('A','decision')};$('#upstreamConditions').onclick=()=>{state.node='resources';conditions()};return;
 }
 const maintenance=k==='maintenance';
 const tabs=`<div class="tabs" role="tablist" aria-label="节点详情">${[['result','完成结果'],['evidence','证据与记录'],['explain','通俗解释']].map(([id,label])=>`<button class="tab ${id===tab?'active':''}" role="tab" aria-selected="${id===tab}" data-result-tab="${id}">${label}</button>`).join('')}</div>`;
 let body;
 if(tab==='result')body=`<div class="resultbox"><h3>${maintenance?'已发现振动异常，建议停用并检查':'质量指标异常，需遏制并复核'}</h3><p>${maintenance?'轴承磨损仍是候选原因，不能据此认定根因。':'CPK 为 1.10，低于当前控制要求。不能据此认定全部产品不合格。'}</p><span class="tag green">${maintenance?'维护初筛':'质量评估'}已完成</span> <span class="tag amber">${maintenance?'现场诊断未完成':'批次复核未完成'}</span></div><h3>已完成了什么</h3><p class="small">${maintenance?'维护诊断工具检查了 CNC-XX 本次告警与历史振动特征，形成初筛回执 M-01。':'质量工具关联了 B 线 CPK 与设备告警，形成评估回执 Q-01。'}</p><div class="facts"><div><span>完成时间 / 尝试</span>08:34 / 第 1 次</div><div><span>执行主体</span>${d[2]}</div></div><h3>下一步由谁做</h3><p class="small">${maintenance?'维修组 09:10 前提交现场检查证据；在核验通过前不得恢复 XX。':'质量组确认风险批次并复检；冻结动作须以仓库回执为准。'}</p><div class="hint">本节点完成只表示这项分析已提交，不表示维修结束、设备恢复或质量放行。</div>`;
 else if(tab==='evidence')body=`<p class="small muted">以下均为样例记录，未连接实际设备或企业系统。</p><div class="evidenceitem"><h3>${maintenance?'维护初筛回执 M-01':'质量评估回执 Q-01'}</h3><p class="small">${d[3]}</p><p class="small muted">模拟工具回执 / 08:34 / 第 1 次</p></div><div class="evidenceitem"><h3>${maintenance?'设备告警 DC-205':'质检记录 DC-206'}</h3><p class="small">${maintenance?'08:23，CNC-XX 上报振动异常。告警不能单独证明损坏部位。':'08:31，B 线连续三批 CPK 下降至 1.10；复核原始测量与取样条件后再判断范围。'}</p><p class="small muted">采集来源：data-collection / 模拟记录</p></div><div class="evidenceitem"><h3>设备与产线关系</h3><p class="small">RelOS 引用设备台账 v12：CNC-XX 归属于 B 线。关联关系可用于选择处置 DAG，不能代替因果验证。</p></div>`;
 else body=`<div class="explain"><h3>${maintenance?'初筛完成 ≠ 设备修好':'评估完成 ≠ 产品放行'}</h3><p>${maintenance?'可以把初筛理解为“先判断哪里值得查”，不是“已经查明并修好”。系统看到异常特征，给出了检查方向。':'系统已发现值得处理的质量信号，但这不等于每件产品都有问题，也不等于复检结束。'}</p><p><b>现在可以做：</b>${maintenance?'安排维修检查，并评估不使用 XX 的保守方案。':'确认批次范围、安排复检，并核对冻结回执。'}</p><p><b>现在不能做：</b>${maintenance?'将轴承磨损写为已确认根因，或直接恢复设备。':'在没有放行证据时解冻库存、恢复发货。'}</p></div><div class="hint">这是与当前结果配套的固定说明，不需要调用模型；有进一步疑问再询问 AI。</div>`;
 panelFrame(d[0]+' · 已完成','先看结论，需要时再看证据或解释。',body,tabs);
 document.querySelectorAll('[data-result-tab]').forEach(b=>b.onclick=()=>nodePanel(k,b.dataset.resultTab));
}
function conditions(){
 panelFrame('条件处置 · 尚缺 2 项','两项生产条件都要满足；可以分别跟进，不需要先打开完整资源台账。',`<div class="notice">方案可评审，但不能开始批量生产。现场结果未核验前，不会因点击或模拟受理自动解锁。</div><div class="condition"><div class="row"><div><span class="tag amber">1 / 刀具未到位</span><h3 style="margin-top:9px">XZ 缺 φ32 铣刀，等待调拨回执</h3><p class="small muted">责任方：仓储组 / 下一次反馈 09:20</p><p class="small">${state.toolAccepted?'已受理，实物交接仍未完成。':'调拨申请待受理，尚不能承诺刀具到位。'}</p></div><button class="btn primary" id="handleTool">处理刀具调拨</button></div></div><div class="condition"><div class="row"><div><span class="tag amber">2 / 首件未核验</span><h3 style="margin-top:9px">备用机首件检验尚未完成</h3><p class="small muted">责任方：质量组 / 工艺、刀具及试制条件满足后安排</p><p class="small">与 C101 复检共用检验组，需要协调时间。</p></div><button class="btn" id="handleInspection">安排首件检验</button></div></div><div class="hint">另有排程约束：XY 仍被订单 B 占用，释放时间待确认。当前不把未确认时间显示为开工承诺。</div>`);
 $('#handleTool').onclick=()=>conditionTask('tool');$('#handleInspection').onclick=()=>conditionTask('inspection');
}
function conditionTask(kind){
 const tool=kind==='tool';
 panelFrame(tool?(state.toolAccepted?'刀具调拨 · 已受理待交接':'刀具调拨 · 等待仓储受理'):'备用机首件检验 · 等待前置条件','插单 A / CNC-XZ / 仍在当前事项工作区',`<button class="btn subtle" id="backConditions">返回条件清单</button><div class="facts"><div><span>责任方</span>${tool?'仓储组':'质量组'}</div><div><span>关联事项</span>插单 A / B 线恢复</div></div>${tool?`<h3>需要调拨什么</h3><p>φ32 铣刀 1 套 → CNC-XZ。必须核对规格、可用状态与交接记录。</p><div class="notice">${state.toolAccepted?'已受理，等待实物交接':'申请尚未受理'}</div><p class="small muted">受理后仍缺发出、接收与可用性确认；不会把“受理”当作刀具已经到位。</p><button class="btn primary" id="acceptTool" ${state.toolAccepted?'disabled':''}>${state.toolAccepted?'仓储已受理 · 演示':'模拟仓储受理'}</button>`:`<h3>放行前必须满足</h3><ul><li>备用机工艺适用性已确认。</li><li>刀具与现场试制条件齐备。</li><li>首件检验有完整记录并通过核验。</li></ul><div class="notice">当前不能确认检验通过。模拟页面不生成真实检测结果。</div><button class="btn" disabled>放行批量生产</button>`}`, '', true);
 $('#backConditions').onclick=conditions;
 if(tool)$('#acceptTool').onclick=()=>{state.toolAccepted=true;render();conditionTask('tool');};
}
function updateChatScope() {
  $('#chatScope').textContent = scope() + (state.node ? ' / ' + nodeData[state.node][0] : '') + ' · 仅当前事项';
}
function openChat() {
  if (!state.work) { toast('请先打开一个事项，再开始讨论。'); return; }
  state.chat = true; $('#chat').hidden = false; $('#main').classList.add('chatopen'); $('#dock').classList.add('chatopen');
  updateChatScope(); renderMessages(); $('#prompt').focus({ preventScroll: true });
}
function closeChat() {
  rememberDraft(); state.chat = false; $('#chat').hidden = true; $('#main').classList.remove('chatopen'); $('#dock').classList.remove('chatopen'); $('#openChat').focus({ preventScroll: true });
}
function renderMessages() {
  const m = $('#messages'); m.replaceChildren();
  const intro = document.createElement('div'); intro.className = 'message';
  intro.innerHTML = `<div class="label">✧ AI 业务助手 · 脚本演示</div><p>当前讨论：<strong>${scope()}</strong>。</p><p>我可以解释进展和比较方案。执行仍以明确批准与业务回执为准。</p>`; m.append(intro);
  for (const x of state.hist[state.case]) {
    const d = document.createElement('div'); d.className = 'message' + (x.user ? ' user' : '');
    const p = document.createElement('p'); p.textContent = x.text; d.append(p);
    if (x.action) {
      const b = document.createElement('button'); b.className = 'btn subtle';
      b.textContent = x.action === 'source' ? '查看流程选择依据 ↗' : x.action === 'decision' ? '进入方案评审 ↗' : '查看当前阻塞 ↗';
      b.onclick = () => { closeChat(); if (x.action === 'source') showSource(); else if (x.action === 'decision') setWorkTab('decision'); else { setWorkTab('conditions'); if (state.case === 'A') showNode('resources'); } }; d.append(b);
    }
    m.append(d);
  }
  m.scrollTop = m.scrollHeight;
}
function send() {
  const q = $('#prompt').value.trim(); if (!q || !state.work) return;
  state.hist[state.case].push({ user: true, text: q });
  let text, action;
  if (/DAG|流程|选择/.test(q)) {
    text = state.case === 'A' ? '系统结合三类扰动，使用 data-collection 记录与 RelOS 的设备—产线线索选择已注册的复合扰动流程。单设备模板覆盖不足，未校验草稿不可执行。选择依据与链路均为模拟。' : '系统依据当前事项的目标与记录选择处置流程；data-collection 提供记录，RelOS 提供关联线索，执行前仍需注册与契约校验。此处为模拟说明。'; action = 'source';
  } else if (/交接/.test(q)) { text = `${cases[state.case].name}：${cases[state.case].next} 下一次承诺：${cases[state.case].update}。接班需明确责任，不能把交接等同于关闭。`; action = 'flow';
  } else if (state.case === 'A' && /方案|批准|同意/.test(q)) { text = '方案 A 不满足质量边界。方案 B 需核实刀具、工艺和首件；聊天不会批准执行，请核对方案 v3 的动作范围。'; action = 'decision';
  } else if (state.case === 'A') { text = '尚缺刀具调拨回执和首件检验确认；XY 还被订单 B 占用。批准不代表实物到位或质量放行，受理不等于完成。以上为模拟状态。'; action = 'flow';
  } else { text = cases[state.case].fact + ' 下一步：' + cases[state.case].next + ' 这是脚本演示，不生成真实执行回执。'; action = 'flow'; }
  state.hist[state.case].push({ text, action }); $('#prompt').value = ''; state.draft[state.case] = ''; renderMessages();
}
$('#openChat').onclick = openChat; $('#closeChat').onclick = closeChat; $('#send').onclick = send;
$('#prompt').oninput = rememberDraft;
$('#prompt').onkeydown = e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); } };
$('#understand').onchange = e => { $('#confirmDecision').disabled = !e.target.checked; };
$('#cancelDecision').onclick = () => $('#decisionDialog').close();
$('#confirmDecision').onclick = () => { if (!$('#understand').checked || state.case !== 'A') return; state.approved = true; $('#decisionDialog').close(); render(); toast('模拟：方案 v3 已批准，现场执行与核验仍待完成。'); };
$('#nodeDialog').addEventListener('close', () => {
  if (!state.work) return;
  const target = document.querySelector(`[data-node="${state.node}"]`) || document.querySelector('.worktabs [aria-pressed="true"]');
  target?.focus({ preventScroll: true });
});
let expanded = false;
$('#expandChat').onclick = () => { expanded = !expanded; document.documentElement.style.setProperty('--chat', expanded ? '560px' : '390px'); };
$('#resize').onpointerdown = e => {
  if (innerWidth <= 1000) return; e.preventDefault();
  const move = e => document.documentElement.style.setProperty('--chat', Math.max(330, Math.min(580, innerWidth - e.clientX)) + 'px');
  const end = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', end); };
  window.addEventListener('pointermove', move); window.addEventListener('pointerup', end);
};
let toastTimer;
function toast(text) { clearTimeout(toastTimer); $('#toast').textContent = text; $('#toast').hidden = false; toastTimer = setTimeout(() => { $('#toast').hidden = true; }, 3500); }
document.addEventListener('keydown', e => { if (e.key === 'Escape' && !document.querySelector('dialog[open]') && state.chat) closeChat(); });
render();
