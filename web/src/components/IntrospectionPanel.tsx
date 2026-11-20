// 业务内省页：结论列表、未推送原因、否决与标注（S6 最小 UI）。
import { useCallback, useEffect, useState } from "react";
import { api, type IntroFinding } from "../api";

export function IntrospectionPanel(props: { onOpenFinding?: (id: string) => void }) {
  const [findings, setFindings] = useState<IntroFinding[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [selected, setSelected] = useState<IntroFinding | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await api.introFindings(false);
      setFindings(r.findings);
      setError(null);
    } catch (e) {
      setError(String(e));
      setFindings([]);
    }
  }, []);

  useEffect(() => {
    load().catch(() => {});
  }, [load]);

  const act = async (id: string, kind: "reject" | "should_push" | "should_not_push") => {
    setBusy(id);
    try {
      if (kind === "reject") await api.introVerdict(id, "rejected", "运营否决");
      else await api.introLabel(id, kind, "");
      await load();
      if (kind === "reject") setSelected(null);
      else {
        const d = await api.introFinding(id);
        setSelected(d.finding);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="intro-panel" data-testid="intro-panel">
      <header className="intro-head">
        <h2>业务内省</h2>
        <button type="button" className="btn small" onClick={() => load()}>
          刷新
        </button>
      </header>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {findings.length === 0 && !error && <p className="muted">暂无结论</p>}
      <ul className="intro-list">
        {findings.map((f) => (
          <li key={f.id}>
            <button
              type="button"
              className={`intro-item ${selected?.id === f.id ? "active" : ""}`}
              data-testid={`intro-finding-${f.id}`}
              onClick={() => {
                setSelected(f);
                props.onOpenFinding?.(f.id);
              }}
            >
              <div className="intro-topic">{f.topic}</div>
              <div className="intro-reason">{f.gate_reason || "（无闸门原因）"}</div>
              <div className="intro-meta">
                {f.verification}
                {f.pushed ? " · 已推送" : " · 未推送"}
              </div>
            </button>
          </li>
        ))}
      </ul>
      {selected && (
        <div className="intro-detail" data-testid="intro-detail">
          <h3>详情</h3>
          <pre className="intro-json">{JSON.stringify(selected.json, null, 2)}</pre>
          <p data-testid="intro-gate-reason">{selected.gate_reason}</p>
          <div className="intro-actions">
            <button
              type="button"
              className="btn small"
              disabled={busy === selected.id}
              data-testid="intro-reject"
              onClick={() => act(selected.id, "reject")}
            >
              否决
            </button>
            <button
              type="button"
              className="btn small"
              disabled={busy === selected.id}
              data-testid="intro-label-push"
              onClick={() => act(selected.id, "should_push")}
            >
              该推
            </button>
            <button
              type="button"
              className="btn small"
              disabled={busy === selected.id}
              data-testid="intro-label-hold"
              onClick={() => act(selected.id, "should_not_push")}
            >
              不该推
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
