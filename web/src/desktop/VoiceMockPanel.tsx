import { useEffect, useRef, useState } from "react";
import { api, type DesktopVoiceSessionDto } from "../api";
import { getAsrBackend, startAsr, type AsrBackendKind, type AsrSession } from "./voiceAsr";
import {
  getVoiceMediaMode,
  requestMicrophone,
  stopMediaStream,
  type VoiceMediaMode,
} from "./voiceMedia";

type Props = {
  taskId: string;
};

export function VoiceMockPanel({ taskId }: Props) {
  const [s, setS] = useState<DesktopVoiceSessionDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<VoiceMediaMode>(() => getVoiceMediaMode());
  const [asrBackend, setAsrBackend] = useState<AsrBackendKind>(() => getAsrBackend());
  const [asrNote, setAsrNote] = useState<string | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const asrRef = useRef<AsrSession | null>(null);
  const lastPartialRef = useRef("");

  const apply = (next: DesktopVoiceSessionDto) => {
    setS(next);
    setError(null);
  };

  const run = (p: Promise<{ session: DesktopVoiceSessionDto }>) => {
    void p.then((r) => apply(r.session)).catch((e: Error) => setError(e.message));
  };

  const stopLocalMedia = () => {
    asrRef.current?.stop();
    asrRef.current = null;
    stopMediaStream(streamRef.current);
    streamRef.current = null;
  };

  useEffect(() => {
    setMode(getVoiceMediaMode());
    setAsrBackend(getAsrBackend());
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
      stopLocalMedia();
    };
  }, [taskId]);

  const openMicReal = async () => {
    setError(null);
    stopLocalMedia();
    const mic = await requestMicrophone();
    if (!mic.ok) {
      run(api.desktopVoicePermission(taskId, "denied"));
      return;
    }
    streamRef.current = mic.stream;
    try {
      const perm = await api.desktopVoicePermission(taskId, "granted");
      apply(perm.session);
      const started = await api.desktopVoiceStart(taskId);
      apply(started.session);
      lastPartialRef.current = "";
      setAsrNote(null);
      asrRef.current = await startAsr(
        {
          onPartial: (text) => {
            lastPartialRef.current = text;
            run(api.desktopVoicePartial(taskId, text));
          },
          onFinal: (text) => {
            lastPartialRef.current = text;
            run(api.desktopVoicePartial(taskId, text));
          },
          onError: (message) => {
            if (message === "remote_asr_unconfigured" || message.startsWith("remote_asr_")) {
              setAsrNote("远端 ASR 未配置或失败，已回退；真钥另验");
            } else {
              setError(message);
            }
          },
        },
        {
          stream: mic.stream,
          onBackend: (kind, detail) => {
            setAsrBackend(kind);
            if (detail === "remote_fallback") {
              setAsrNote("远端 ASR 未配置或失败，已回退 Web Speech；真钥另验");
            }
          },
        },
      );
    } catch (e) {
      stopLocalMedia();
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const injectAsrPartial = (text: string) => {
    if (asrRef.current?.injectPartial) {
      asrRef.current.injectPartial(text);
      return;
    }
    lastPartialRef.current = text;
    run(api.desktopVoicePartial(taskId, text));
  };

  const commitFinal = (text: string, link: boolean) => {
    asrRef.current?.stop();
    asrRef.current = null;
    run(api.desktopVoiceFinalize(taskId, text, link));
  };

  if (!s) {
    return (
      <div className="voice-mock" data-testid="voice-mock-panel" data-media-mode={mode}>
        <p className="muted">加载语音会话…</p>
        {error && (
          <p className="evidence-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  const isReal = mode === "real";

  return (
    <div
      className="voice-mock"
      data-testid="voice-mock-panel"
      data-media-mode={mode}
    >
      <h3>{isReal ? "语音（真机媒体）" : "语音（fixture）"}</h3>
      <p className="muted" data-testid="voice-media-mode">
        media={mode}
      </p>
      <p className="muted" data-testid="voice-asr-backend">
        asr={asrBackend}
      </p>
      {asrNote && (
        <p className="muted" data-testid="voice-asr-note">
          {asrNote}
        </p>
      )}
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
          onClick={() => {
            stopLocalMedia();
            run(api.desktopVoiceReset(taskId));
          }}
        >
          重置
        </button>
        {!isReal && (
          <>
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
          </>
        )}
        {isReal && (
          <>
            <button
              type="button"
              className="btn primary small"
              data-testid="voice-open-mic"
              onClick={() => void openMicReal()}
            >
              开启麦克风
            </button>
            <button
              type="button"
              className="btn small"
              data-testid="voice-asr-partial"
              onClick={() => injectAsrPartial("正在说…SMT-03 振动")}
            >
              ASR 注入部分
            </button>
          </>
        )}
        <button
          type="button"
          className="btn small"
          data-testid="voice-commit"
          onClick={() => {
            // fixture：保持既有 E2E-ADVOICE 固定最终文案；real：用 ASR/partial
            const text = isReal
              ? lastPartialRef.current || s.partial || "调查 SMT-03 振动异常"
              : "调查 SMT-03 振动异常";
            commitFinal(text, true);
          }}
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
          onClick={() => {
            stopLocalMedia();
            run(api.desktopVoiceOffline(taskId));
          }}
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
