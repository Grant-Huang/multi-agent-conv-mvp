import { useEffect, useState } from "react";
import { api, type DesktopEvidenceDto } from "../api";
import { loadPrincipal, savePrincipal, type DesktopPrincipal } from "./principalStore";

export type EvidenceLayer = "list" | "preview" | "source";

type Props = {
  taskId: string;
  layer: EvidenceLayer;
  evidenceId: string | null;
  /** 演示空列表：传入 empty（仍本地，不打 API） */
  mode?: "normal" | "empty";
  onLayer: (layer: EvidenceLayer, evidenceId: string | null) => void;
  onClose: () => void;
  onDemoEmpty?: () => void;
  /** UXFIX-B（INTENT-003）：服务端告知的测试模式；仅在 true 时渲染身份切换与夹具控件 */
  testFixtures?: boolean;
};

export function EvidencePanel({ taskId, layer, evidenceId, mode = "normal", onLayer, onClose, onDemoEmpty, testFixtures = false }: Props) {
  const [principal, setPrincipal] = useState<DesktopPrincipal>(() => loadPrincipal());
  const [list, setList] = useState<DesktopEvidenceDto[]>([]);
  const [detail, setDetail] = useState<DesktopEvidenceDto | null>(null);
  const [denied, setDenied] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState<string | null>(null);
  const [searchHits, setSearchHits] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const switchPrincipal = (p: DesktopPrincipal) => {
    savePrincipal(p);
    setPrincipal(p);
    setDenied(null);
    setUnavailable(null);
    setDetail(null);
    if (layer !== "list") onLayer("list", null);
  };

  useEffect(() => {
    if (mode === "empty") {
      setList([]);
      return;
    }
    let cancelled = false;
    setBusy(true);
    setErr(null);
    api
      .desktopEvidenceList(taskId, principal)
      .then((r) => {
        if (!cancelled) setList(r.evidence);
      })
      .catch((e: Error) => {
        if (!cancelled) setErr(e.message);
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [taskId, principal, mode]);

  useEffect(() => {
    if (mode === "empty" || !evidenceId || layer === "list") {
      setDetail(null);
      setDenied(null);
      setUnavailable(null);
      return;
    }
    const view = layer === "source" ? "source" : "summary";
    let cancelled = false;
    setBusy(true);
    api.desktopEvidenceGet(evidenceId, principal, view).then((r) => {
      if (cancelled) return;
      if (r.ok) {
        setDetail(r.evidence);
        setDenied(null);
        setUnavailable(null);
      } else if (r.status === 403) {
        setDetail(null);
        setDenied(r.message);
        setUnavailable(null);
      } else if (r.status === 410) {
        setDetail(null);
        setDenied(null);
        setUnavailable(r.message);
      } else {
        setErr(r.message);
      }
    }).finally(() => {
      if (!cancelled) setBusy(false);
    });
    return () => {
      cancelled = true;
    };
  }, [evidenceId, layer, principal, mode]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (layer === "source") onLayer("preview", evidenceId);
      else if (layer === "preview") onLayer("list", null);
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layer, evidenceId, onClose, onLayer]);

  const runSearch = async () => {
    const r = await api.desktopEvidenceSearch("首件", principal);
    setSearchHits(r.evidence.map((e) => e.id));
  };

  const revokeCurrent = async () => {
    if (!evidenceId) return;
    await api.desktopEvidenceRevoke(evidenceId, principal);
    setDenied("无权访问该证据");
    setDetail(null);
    const r = await api.desktopEvidenceList(taskId, principal);
    setList(r.evidence);
  };

  return (
    <div className="desktop-panel evidence-panel" data-testid="evidence-panel" role="dialog" aria-label="证据">
      <header className="desktop-panel-head">
        <button type="button" className="btn small" data-testid="evidence-back" onClick={() => {
          if (layer === "source") onLayer("preview", evidenceId);
          else if (layer === "preview") onLayer("list", null);
          else onClose();
        }}>
          {layer === "list" ? "关闭证据" : "返回上层"}
        </button>
        <h2>证据</h2>
        <span className="desktop-sim" data-testid="evidence-mode-label">真实 ACL</span>
      </header>

      {/* UXFIX-B：身份切换只在测试模式渲染（服务端告知），正常界面不出现（CH-U5） */}
      {testFixtures && (
        <div className="evidence-acl-bar" data-testid="evidence-acl-bar">
        <span>身份</span>
        {(["alice", "bob", "carol"] as DesktopPrincipal[]).map((p) => (
          <button
            key={p}
            type="button"
            className={principal === p ? "btn small active" : "btn small"}
            data-testid={`evidence-principal-${p}`}
            aria-pressed={principal === p}
            onClick={() => switchPrincipal(p)}
          >
            {p}
          </button>
        ))}
        <button type="button" className="btn small" data-testid="evidence-search-acl" onClick={() => void runSearch()}>
          搜索「首件」
        </button>
        {searchHits && (
          <span data-testid="evidence-search-hits">
            {searchHits.length === 0 ? "none" : searchHits.join(",")}
          </span>
        )}
        </div>
      )}

      {err && (
        <p className="evidence-error" role="alert" data-testid="evidence-load-error">
          {err}
        </p>
      )}
      {busy && <p className="muted" data-testid="evidence-loading">加载中…</p>}

      {layer === "list" && (
        <div data-testid="evidence-list">
          {list.length === 0 ? (
            <p className="desktop-panel-empty" data-testid="evidence-empty" role="status">
              暂无证据记录
            </p>
          ) : (
            <ul className="evidence-list">
              {list.map((e) => (
                <li key={e.id}>
                  <button
                    type="button"
                    className="evidence-item"
                    data-testid={`evidence-item-${e.id}`}
                    onClick={() => onLayer("preview", e.id)}
                  >
                    <span className="evidence-title">{e.title}</span>
                    <span className="evidence-meta">
                      {e.object_label} · {e.version} · {e.status}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {testFixtures && onDemoEmpty && mode !== "empty" && (
            <button type="button" className="btn small" data-testid="evidence-demo-empty" onClick={onDemoEmpty}>
              空列表夹具
            </button>
          )}
          {testFixtures && mode === "empty" && (
            <button type="button" className="btn small" data-testid="evidence-demo-restore" onClick={() => onClose()}>
              关闭空夹具
            </button>
          )}
        </div>
      )}

      {layer === "preview" && denied && (
        <p className="evidence-error" data-testid="evidence-denied" role="alert">
          {denied}
        </p>
      )}
      {layer === "preview" && unavailable && (
        <p className="evidence-error" data-testid="evidence-expired" role="alert">
          {unavailable}
        </p>
      )}
      {layer === "preview" && detail && (
        <div data-testid="evidence-preview">
          <h3>{detail.title}</h3>
          <dl className="evidence-dl">
            <dt>业务对象</dt>
            <dd data-testid="evidence-object">{detail.object_label}</dd>
            <dt>采集时间</dt>
            <dd data-testid="evidence-time">{detail.collected_at}</dd>
            <dt>来源类型</dt>
            <dd>{detail.source_type}</dd>
            <dt>版本</dt>
            <dd data-testid="evidence-version">{detail.version}</dd>
            <dt>任务</dt>
            <dd data-testid="evidence-task">{detail.task_id}</dd>
          </dl>
          {detail.status === "failed" && (
            <p className="evidence-error" data-testid="evidence-error" role="alert">
              {detail.error_message}
            </p>
          )}
          {detail.status === "expired" && (
            <p className="evidence-error" data-testid="evidence-expired" role="alert">
              {detail.error_message}
            </p>
          )}
          {detail.status === "ok" && detail.summary && (
            <p data-testid="evidence-summary">{detail.summary}</p>
          )}
          {detail.status !== "failed" && (
            <button
              type="button"
              className="btn small"
              data-testid="evidence-open-source"
              onClick={() => onLayer("source", detail.id)}
            >
              查看来源原文
            </button>
          )}
          <button type="button" className="btn small" data-testid="evidence-revoke" onClick={() => void revokeCurrent()}>
            撤权当前身份
          </button>
        </div>
      )}

      {layer === "source" && denied && (
        <p className="evidence-error" data-testid="evidence-denied" role="alert">
          {denied}
        </p>
      )}
      {layer === "source" && unavailable && (
        <p className="evidence-error" data-testid="evidence-expired" role="alert">
          {unavailable}
        </p>
      )}
      {layer === "source" && detail && (
        <div data-testid="evidence-source">
          <h3>来源原文</h3>
          <pre className="evidence-source-body">{detail.source_body || "（无原文）"}</pre>
        </div>
      )}
    </div>
  );
}
