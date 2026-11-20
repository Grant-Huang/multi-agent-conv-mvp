import { useState } from "react";
import {
  bargeIn,
  commitFinal,
  continueAsText,
  goOffline,
  loadVoiceSession,
  markConnected,
  resetVoice,
  setPartial,
  setPermission,
  setShareTranscript,
  startListening,
  startPlayback,
  type VoiceSession,
} from "./voiceMock";

export function VoiceMockPanel() {
  const [s, setS] = useState<VoiceSession>(() => resetVoice());

  const apply = (next: VoiceSession) => setS(next);

  return (
    <div className="voice-mock" data-testid="voice-mock-panel">
      <h3>语音（模拟）</h3>
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
        <dd data-testid="voice-final">{s.finalText || "—"}</dd>
        <dt>关联任务</dt>
        <dd data-testid="voice-link">{s.linkedTaskId ?? "未关联"}</dd>
        <dt>转写共享</dt>
        <dd data-testid="voice-share">{s.shareTranscript ? "已共享" : "未授权不共享"}</dd>
      </dl>
      <div className="settings-actions">
        <button type="button" className="btn small" data-testid="voice-reset" onClick={() => apply(resetVoice())}>
          重置
        </button>
        <button type="button" className="btn small" data-testid="voice-deny" onClick={() => apply(setPermission("denied"))}>
          拒绝麦克风
        </button>
        <button
          type="button"
          className="btn primary small"
          data-testid="voice-start"
          onClick={() => {
            let next = startListening();
            if (next.phase === "connecting") next = markConnected();
            apply(next);
          }}
        >
          开启麦克风
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-inject-partial"
          onClick={() => apply(setPartial("正在说…SMT"))}
        >
          注入部分转写
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-commit"
          onClick={() => apply(commitFinal("调查 SMT-03 振动异常", true))}
        >
          确认最终转写并关联
        </button>
        <button type="button" className="btn small" data-testid="voice-play" onClick={() => apply(startPlayback())}>
          开始播报
        </button>
        <button type="button" className="btn small" data-testid="voice-barge" onClick={() => apply(bargeIn())}>
          插话停止播报
        </button>
        <button type="button" className="btn small" data-testid="voice-offline" onClick={() => apply(goOffline())}>
          断线
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-as-text"
          onClick={() => apply(continueAsText(loadVoiceSession().finalText || "改用文字继续"))}
        >
          改用文字继续
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="voice-share-toggle"
          aria-pressed={s.shareTranscript}
          onClick={() => apply(setShareTranscript(!s.shareTranscript))}
        >
          {s.shareTranscript ? "撤销转写共享" : "授权共享转写"}
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
