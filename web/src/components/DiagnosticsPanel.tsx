/// <reference types="vite/client" />
// 诊断页：模型连通性、执行组状态、按错误码统计、最近的问题。
// "复制诊断信息"会复制完整 JSON，可以直接贴给 AI 或开发者排查。
import { useEffect, useState } from "react";
import { api, type Diagnostics } from "../api";
import { refreshStats } from "../refresh";

/** 一个槽位的状态：占用者、是否正在生成。不显示 llama 的 id_task：它是服务端累计的请求编号，不是任务数，也不是 token 数 */
export function SlotRow({ slot }: { slot: Diagnostics["backends"][number]["slots"][number] }) {
  return (
    <div className="kv">
      <span className={`dot ${slot.server_busy || slot.holder ? "bad" : "ok"}`} />
      槽位 {slot.id}（{slot.class}）
      {slot.holder ? ` 占用：${slot.holder}` : " 空闲"}
      {slot.server_busy ? " · 正在生成" : ""}
    </div>
  );
}

export function DiagnosticsPanel({ refreshTick }: { refreshTick: number }) {
  const [d, setD] = useState<Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const load = () => api.diagnostics().then(setD).catch((e) => setError(String(e)));

  // 打开页签时拉一次；之后只在状态变化 / 助手消息时节流刷新
  useEffect(() => {
    load();
  }, [refreshTick]);

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
        <button className="btn small" onClick={load}>刷新</button>
        <button className="btn small" onClick={copy}>{copied ? "已复制" : "复制诊断信息"}</button>
        <span className="kv muted push-right">v{d.version}</span>
      </div>
      {import.meta.env.DEV && (
        <div className="kv muted">刷新触发 {refreshStats.fired} 次，节流丢弃 {refreshStats.dropped} 次</div>
      )}

      <section className="section">
        <h4>后端</h4>
        {(d.backends ?? []).map((b) => (
          <div key={`${b.name}${b.base_url}`} className="task-card log">
            <div className="kv">
              <span className={`dot ${b.ok ? "ok" : "bad"}`} />
              <b>{b.name}</b>（{b.kind}）{b.model}
              <span className="muted"> · {b.roles.join("、")}</span>
            </div>
            <div className="kv muted">{b.base_url}</div>
            {!b.ok && <div className="kv bad-text">{b.error}</div>}
            {b.server_error && <div className="kv bad-text">{b.server_error}</div>}
            {b.metrics && (
              <div className="kv">
                处理中 {b.metrics.processing}，排队 {b.metrics.queued}，生成 {b.metrics.gen_per_sec.toFixed(1)} token/s
              </div>
            )}
            {b.slots.map((s) => (
              <SlotRow key={s.id} slot={s} />
            ))}
            <div className="kv muted">
              调度：在途 {b.scheduler.inflight}
              {b.scheduler.max_inflight > 0 ? `/${b.scheduler.max_inflight}` : ""}，排队 {b.scheduler.queue_len}
              {b.scheduler.paused_ms > 0 ? `，限速暂停 ${Math.ceil(b.scheduler.paused_ms / 1000)}s` : ""}
            </div>
            <div className="kv muted">
              最近 {b.calls.total} 次调用：耗时 P50 {b.calls.total_ms_p50}ms / P95 {b.calls.total_ms_p95}ms，首 token P50 {b.calls.ttft_ms_p50}ms / P95 {b.calls.ttft_ms_p95}ms
              {Object.keys(b.calls.outcomes).length > 0 ? `（${Object.entries(b.calls.outcomes).map(([k, n]) => `${k} ${n}`).join("，")}）` : ""}
            </div>
          </div>
        ))}
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
