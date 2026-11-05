// 诊断页：模型连通性、执行组状态、按错误码统计、最近的问题。
// "复制诊断信息"会复制完整 JSON，可以直接贴给 AI 或开发者排查。
import { useEffect, useState } from "react";
import { api, type Diagnostics } from "../api";

export function DiagnosticsPanel({ eventTick }: { eventTick: number }) {
  const [d, setD] = useState<Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    api.diagnostics().then(setD).catch((e) => setError(String(e)));
  }, [eventTick]);

  const copy = async () => {
    if (!d) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(d, null, 2));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setError("复制失败：浏览器不允许访问剪贴板");
    }
  };

  if (error) return <div className="panel-empty bad-text">{error}</div>;
  if (!d) return <div className="panel-empty">加载中…</div>;

  const counts = Object.entries(d.problem_counts).sort((a, b) => b[1] - a[1]);

  return (
    <div className="task-list">
      <div className="row">
        <button className="btn small" onClick={copy}>{copied ? "已复制" : "复制诊断信息"}</button>
        <span className="kv muted push-right">v{d.version}</span>
      </div>

      <section className="section">
        <h4>模型</h4>
        <div className="kv">
          <span className={`dot ${d.llm.ok ? "ok" : "bad"}`} />
          {d.llm.ok ? `已连接（${(d.llm.models ?? []).join(", ") || "未报告模型名"}）` : `${d.llm.code}：${d.llm.error}`}
        </div>
      </section>

      <section className="section">
        <h4>执行组</h4>
        {Object.entries(d.groups).map(([role, g]) => (
          <div key={role} className="kv">
            <span className={`dot ${g.enabled ? "ok" : "bad"}`} />
            <b>{role}</b>：{g.error ? <span className="bad-text">{g.error}</span> : (g.members ?? []).join(" → ") || "无"}
          </div>
        ))}
      </section>

      <section className="section">
        <h4>问题统计（最近 500 条）</h4>
        {counts.length === 0 ? (
          <div className="kv muted">没有问题。</div>
        ) : (
          counts.map(([code, n]) => (
            <div key={code} className="kv">
              <code>{code}</code> × {n}
            </div>
          ))
        )}
      </section>

      <section className="section">
        <h4>最近的问题</h4>
        {d.recent_problems.length === 0 && <div className="kv muted">没有。</div>}
        {d.recent_problems.slice(0, 30).map((p, i) => (
          <div key={i} className="task-card log">
            <div className="kv">
              <span className={p.level === "ERROR" ? "bad-text" : "warn-text"}>{p.level}</span>{" "}
              <code>{p.code ?? "-"}</code> {p.event} <span className="muted">{p.time.slice(11, 19)}</span>
            </div>
            <div className="kv">{p.message}</div>
            {Object.keys(p.fields).length > 0 && (
              <div className="kv muted">
                {Object.entries(p.fields)
                  .map(([k, v]) => `${k}=${v}`)
                  .join("  ")}
              </div>
            )}
          </div>
        ))}
      </section>
    </div>
  );
}
