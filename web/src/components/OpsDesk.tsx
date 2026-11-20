// 运营桌面看板与详情抽屉（docs/mom/OPS_DESK_TASKS.md D1-3）。只读；批准不在这里。
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { api, type OpsDeskCaseDetail, type OpsDeskRow, type OpsDeskView } from "../api";

const FILTER_KEY = "ops-desk-filter";
type Filter = "all" | "pending" | "blocked" | "risk";

function readFilter(): Filter {
  try {
    const v = localStorage.getItem(FILTER_KEY);
    if (v === "pending" || v === "blocked" || v === "risk" || v === "all") return v;
  } catch {
    /* ignore */
  }
  return "all";
}

function writeFilter(v: Filter) {
  try {
    localStorage.setItem(FILTER_KEY, v);
  } catch {
    /* ignore */
  }
}

function textOrNone(v: unknown): string {
  if (v == null || v === "") return "暂无";
  if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return String(v);
  try {
    return JSON.stringify(v);
  } catch {
    return "暂无";
  }
}

function missingLine(detail: OpsDeskCaseDetail): string {
  const stage = String(detail.stage ?? "");
  if (stage === "WAITING_HUMAN") return "等待批准人决定";
  if (stage === "BLOCKED") return "安全联锁生效，需先解除";
  const alerts = Array.isArray(detail.alerts) ? detail.alerts : [];
  const breaches = Array.isArray(detail.breaches) ? detail.breaches : [];
  if (alerts.length || breaches.length) {
    return [...breaches, ...alerts].map(String).join("、");
  }
  return "暂无";
}

function proposalSummary(detail: OpsDeskCaseDetail): string {
  const p = detail.proposal;
  if (!p || typeof p !== "object") return "暂无";
  const obj = p as Record<string, unknown>;
  for (const key of ["summary", "title", "action", "plan"]) {
    if (obj[key] != null && obj[key] !== "") return textOrNone(obj[key]);
  }
  return textOrNone(p);
}

function progressLine(detail: OpsDeskCaseDetail): string {
  const dag = detail.dag;
  let done = 0;
  let active = 0;
  if (dag && typeof dag === "object") {
    for (const n of Object.values(dag as Record<string, { status?: string }>)) {
      const s = n?.status ?? "";
      if (s === "DONE" || s === "VERIFIED") done += 1;
      else if (s === "RUNNING" || s === "READY") active += 1;
    }
  }
  const deadlines = detail.deadlines;
  let next = "暂无";
  if (deadlines && typeof deadlines === "object") {
    const vals = Object.values(deadlines as Record<string, number>).filter((x) => typeof x === "number");
    if (vals.length) next = String(Math.min(...vals));
  }
  return `已完成步骤 ${done}，进行中 ${active}；最近截止点：${next}`;
}

function evidenceCount(detail: OpsDeskCaseDetail): number {
  const e = detail.evidence;
  if (e && typeof e === "object") return Object.keys(e as object).length;
  return 0;
}

export function OpsDesk(props: { onOpenChannel: (caseId: string) => void }) {
  const [view, setView] = useState<OpsDeskView | null>(null);
  const [filter, setFilter] = useState<Filter>(readFilter);
  const [completedOpen, setCompletedOpen] = useState(false);
  const [detail, setDetail] = useState<OpsDeskCaseDetail | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const rowRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const focusBack = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    api
      .opsDesk()
      .then((v) => {
        setView(v);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    if (!detailId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeDrawer();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [detailId]);

  const setFilterPersist = (f: Filter) => {
    setFilter(f);
    writeFilter(f);
  };

  const openDrawer = async (row: OpsDeskRow) => {
    focusBack.current = row.case_id;
    try {
      const d = await api.opsDeskCase(row.case_id);
      setDetail(d);
      setDetailId(row.case_id);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const closeDrawer = () => {
    const id = focusBack.current;
    setDetail(null);
    setDetailId(null);
    if (id) {
      requestAnimationFrame(() => rowRefs.current[id]?.focus());
    }
  };

  const openInChannel = async (row: OpsDeskRow, e: MouseEvent) => {
    e.stopPropagation();
    try {
      await api.opsOpenInChannel(row.case_id);
      props.onOpenChannel(row.case_id);
    } catch (err) {
      setError(String(err));
    }
  };

  const strip = view?.strip ?? { pending: 0, blocked: 0, overdue: 0, risk: 0, completed: 0 };
  const all = view?.rows ?? [];
  const completed = all.filter((r) => r.group === "completed");
  const active = all.filter((r) => r.group !== "completed");
  const filtered = active.filter((r) => {
    if (filter === "all") return true;
    if (filter === "pending") return r.group === "pending";
    if (filter === "blocked") return r.group === "blocked";
    if (filter === "risk") return r.group === "risk" || r.group === "overdue";
    return true;
  });
  const shown = completedOpen ? [...filtered, ...completed] : filtered;

  return (
    <div className="ops-desk">
      {error && (
        <div className="error-bar" role="alert" onClick={() => setError(null)}>
          {error}（点击关闭）
        </div>
      )}
      {view?.state === "no_data" && <p className="ops-empty">暂无数据</p>}
      <div className="ops-strip" data-testid="ops-strip">
        待批准 {strip.pending} · 受阻 {strip.blocked}
        {strip.overdue > 0 ? ` · 超时 ${strip.overdue}` : ""}
        {strip.risk > 0 ? ` · 风险 ${strip.risk}` : ""}
      </div>
      <div className="ops-filters" role="group" aria-label="筛选">
        {(
          [
            ["all", "全部"],
            ["pending", "待我批准"],
            ["blocked", "受阻/需协助"],
            ["risk", "风险提示"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            className="btn small"
            aria-pressed={filter === id}
            onClick={() => setFilterPersist(id)}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="ops-rows">
        {shown.map((row) => (
          <div
            key={row.case_id}
            className={`ops-row ${row.group === "pending" || row.group === "blocked" ? "level-p0" : ""}`}
            data-testid="ops-row"
            tabIndex={0}
            ref={(el) => {
              rowRefs.current[row.case_id] = el;
            }}
            onClick={() => openDrawer(row)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                openDrawer(row);
              }
            }}
          >
            <div className="ops-row-main">
              <div className="ops-row-title">
                {row.title} <span className="badge sim">模拟</span>
              </div>
              <div className="ops-row-meta">
                {row.case_id} · {row.stage_label}
                {row.stale ? " · 证据可能过期" : ""}
              </div>
            </div>
            {row.group === "pending" ? (
              <button type="button" className="btn small" onClick={(e) => openInChannel(row, e)}>
                在频道中处理
              </button>
            ) : (
              <button
                type="button"
                className="btn small"
                onClick={(e) => {
                  e.stopPropagation();
                  openDrawer(row);
                }}
              >
                查看详情
              </button>
            )}
          </div>
        ))}
      </div>
      {completed.length > 0 && (
        <button
          type="button"
          className="btn small ops-completed-toggle"
          aria-expanded={completedOpen}
          onClick={() => setCompletedOpen((v) => !v)}
        >
          已完成 ({completed.length})
        </button>
      )}

      {detail && detailId && (
        <div className="ops-drawer" role="dialog" aria-label="事项详情" aria-modal="true">
          <div className="ops-drawer-head">
            <strong>事项详情</strong>
            <span className="badge sim">模拟数据</span>
            <button type="button" className="btn small" onClick={closeDrawer}>
              关闭
            </button>
          </div>
          <section>
            <h3>发生了什么与影响</h3>
            <p>
              阶段：{textOrNone(detail.stage)}；场景 {textOrNone(detail.scenario)}；客户{" "}
              {textOrNone(detail.customer)}；批次 {textOrNone(detail.batch)}；约束：
              {Array.isArray(detail.constraints) && detail.constraints.length
                ? detail.constraints.map(String).join("、")
                : "暂无"}
            </p>
          </section>
          <section>
            <h3>责任</h3>
            <p>
              负责人：调度；决策箱：{textOrNone(detail.decision_inbox)}
            </p>
          </section>
          <section>
            <h3>方案与前提</h3>
            <p>
              {proposalSummary(detail)}；方案版本 {textOrNone(detail.plan_revision)}；限制：
              {Array.isArray(detail.limitations) && detail.limitations.length
                ? detail.limitations.map(String).join("；")
                : "暂无"}
            </p>
          </section>
          <section>
            <h3>缺什么</h3>
            <p>{missingLine(detail)}</p>
          </section>
          <section>
            <h3>进展与下次反馈</h3>
            <p>{progressLine(detail)}</p>
          </section>
          <section>
            <h3>凭什么算解决</h3>
            <p>核验证据齐全后才能关闭；已有证据 {evidenceCount(detail)} 条。</p>
          </section>
          <details>
            <summary>原始数据</summary>
            <pre>{JSON.stringify(detail)}</pre>
          </details>
        </div>
      )}
    </div>
  );
}
