import { useEffect, useState } from "react";
import { api, type DesktopHandoffDto } from "../api";

type Props = {
  taskId: string;
};

export function HandoffMockPanel({ taskId }: Props) {
  const [s, setS] = useState<DesktopHandoffDto | null>(null);
  const [phoneDraft, setPhoneDraft] = useState("");
  const [desktopDraft, setDesktopDraft] = useState("");
  const [error, setError] = useState<string | null>(null);

  const apply = (next: DesktopHandoffDto) => {
    setS(next);
    setPhoneDraft(next.phone_text);
    setDesktopDraft(next.desktop_text);
    setError(null);
  };

  const run = (p: Promise<{ handoff: DesktopHandoffDto }>) => {
    void p.then((r) => apply(r.handoff)).catch((e: Error) => setError(e.message));
  };

  useEffect(() => {
    let cancelled = false;
    api
      .desktopHandoffReset(taskId)
      .then((r) => {
        if (!cancelled) apply(r.handoff);
      })
      .catch((e: Error) => {
        if (!cancelled) setError(e.message);
      });
    return () => {
      cancelled = true;
    };
  }, [taskId]);

  if (!s) {
    return (
      <div className="handoff-mock" data-testid="handoff-mock-panel">
        <p className="muted">加载跨端状态…</p>
        {error && (
          <p className="evidence-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="handoff-mock" data-testid="handoff-mock-panel">
      <h3>跨端续接（fixture）</h3>
      <dl className="evidence-dl">
        <dt>Task</dt>
        <dd data-testid="handoff-task">{s.task_id}</dd>
        <dt>Run</dt>
        <dd data-testid="handoff-run">{s.run_id}</dd>
        <dt>阅读锚点</dt>
        <dd data-testid="handoff-anchor">{s.read_message_id}</dd>
        <dt>冲突</dt>
        <dd data-testid="handoff-conflict">{s.conflict ? "有" : "无"}</dd>
        <dt>选用</dt>
        <dd data-testid="handoff-chosen">{s.chosen ?? "—"}</dd>
        <dt>语音转写共享</dt>
        <dd data-testid="handoff-voice-shared">{s.voice_shared ? "共享中" : "未授权不共享"}</dd>
      </dl>
      {error && (
        <p className="evidence-error" role="alert">
          {error}
        </p>
      )}
      <label className="settings-field">
        手机草稿
        <input
          data-testid="handoff-phone"
          value={phoneDraft}
          onChange={(e) => {
            const text = e.target.value;
            setPhoneDraft(text);
            void api
              .desktopHandoffDraft(taskId, "phone", text)
              .then((r) => {
                setS(r.handoff);
                setDesktopDraft(r.handoff.desktop_text);
                setError(null);
              })
              .catch((err: Error) => setError(err.message));
          }}
        />
      </label>
      <label className="settings-field">
        桌面草稿
        <input
          data-testid="handoff-desktop"
          value={desktopDraft}
          onChange={(e) => {
            const text = e.target.value;
            setDesktopDraft(text);
            void api
              .desktopHandoffDraft(taskId, "desktop", text)
              .then((r) => {
                setS(r.handoff);
                setPhoneDraft(r.handoff.phone_text);
                setError(null);
              })
              .catch((err: Error) => setError(err.message));
          }}
        />
      </label>
      <p className="muted">
        手机 v{s.phone_version} · 桌面 v{s.desktop_version}
      </p>
      <div className="settings-actions">
        <button
          type="button"
          className="btn small"
          data-testid="handoff-reset"
          onClick={() => run(api.desktopHandoffReset(taskId))}
        >
          重置
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-choose-phone"
          disabled={!s.conflict}
          onClick={() => run(api.desktopHandoffResolve(taskId, "phone"))}
        >
          采用手机稿
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-choose-desktop"
          disabled={!s.conflict}
          onClick={() => run(api.desktopHandoffResolve(taskId, "desktop"))}
        >
          采用桌面稿
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-voice-share"
          aria-pressed={s.voice_shared}
          onClick={() => run(api.desktopHandoffShareVoice(taskId, !s.voice_shared))}
        >
          {s.voice_shared ? "关闭转写共享" : "授权转写共享"}
        </button>
      </div>
    </div>
  );
}
