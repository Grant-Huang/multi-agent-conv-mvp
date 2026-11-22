import { useEffect, useState } from "react";
import { api, type DesktopVoiceSessionDto } from "../api";

type Props = {
  taskId: string;
};

export function VoiceMockPanel({ taskId }: Props) {
  const [s, setS] = useState<DesktopVoiceSessionDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  const apply = (next: DesktopVoiceSessionDto) => {
    setS(next);
    setError(null);
  };

  const run = (p: Promise<{ session: DesktopVoiceSessionDto }>) => {
    void p.then((r) => apply(r.session)).catch((e: Error) => setError(e.message));
  };

  useEffect(() => {
    let cancelled = false;
    api
      .desktopVoiceReset(taskId)
      .then((r) => {
        if (!cancelled) apply(r.session);
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
      <div className="voice-mock" data-testid="voice-mock-panel">
        <p className="muted">加载语音会话…</p>
        {error && (
          <p className="evidence-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="voice-mock" data-testid="voice-mock-panel">
      <h3>语音（fixture）</h3>
      <dl className="evidence-dl">
        <dt>权限</dt>
        <dd data-testid="voice-permission">{s.permission}</dd>
        <dt>相位</dt>
        <dd data-testid="voice-phase">{s.phase}</dd>
        <dt>播报</dt>
        <dd data-testid="voice-playing">{s.playing ? "播放中" : "已停"}</dd>
        <dt>部分转写</dt>
        <dd data-testid="voice-partial">{s.partial || "—"}</dd>
        <dt>最终转写</dt>
        <dd data-testid="voice-final">{s.final_text || "—"}</dd>
        <dt>关联任务</dt>
        <dd data-testid="voice-link">{s.linked_task_id ?? "未关联"}</dd>
        <dt>转写共享</dt>
        <dd data-testid="voice-share">{s.share_transcript ? "已共享" : "未授权不共享"}</dd>
      </dl>
      {error && (
        <p className="evidence-error" role="alert">
          {error}
        </p>
      )}
      <div className="settings-actions">
        <button
          type="button"
          className="btn small"
          data-testid="voice-reset"
          onClick={() => run(api.desktopVoiceReset(taskId))}
        >
          重置
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-deny"
          onClick={() => run(api.desktopVoicePermission(taskId, "denied"))}
        >
          拒绝麦克风
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="voice-start"
          onClick={() => run(api.desktopVoiceStart(taskId))}
        >
          开启麦克风
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-inject-partial"
          onClick={() => run(api.desktopVoicePartial(taskId, "正在说…SMT"))}
        >
          注入部分转写
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-commit"
          onClick={() => run(api.desktopVoiceFinalize(taskId, "调查 SMT-03 振动异常", true))}
        >
          确认最终转写并关联
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-play"
          onClick={() => run(api.desktopVoicePlay(taskId))}
        >
          开始播报
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-barge"
          onClick={() => run(api.desktopVoiceBarge(taskId))}
        >
          插话停止播报
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-offline"
          onClick={() => run(api.desktopVoiceOffline(taskId))}
        >
          断线
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-as-text"
          onClick={() =>
            run(api.desktopVoiceAsText(taskId, s.final_text || "改用文字继续"))
          }
        >
          改用文字继续
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-share-toggle"
          aria-pressed={s.share_transcript}
          onClick={() => run(api.desktopVoiceShare(taskId, !s.share_transcript))}
        >
          {s.share_transcript ? "撤销转写共享" : "授权共享转写"}
        </button>
      </div>
      {(s.permission === "denied" || s.phase === "offline") && (
        <p className="muted" data-testid="voice-fallback">
          可继续用文字输入，不依赖麦克风
        </p>
      )}
    </div>
  );
}
