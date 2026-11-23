/**
 * ASR 适配器调度（T-75/T-76）：
 * - webspeech（默认）：浏览器 SpeechRecognition + Harness 注入
 * - remote-stub：远端合同回放，无密钥
 * - remote：可选远端；失败回退 webspeech
 * 仅产出文本；服务权威仍由调用方 POST /voice/partial|finalize。
 */

import {
  startRemoteStubAsr,
  tryStartRemoteLiveAsr,
} from "./voiceRemoteAsr";

export type AsrHandlers = {
  onPartial?: (text: string) => void;
  onFinal?: (text: string) => void;
  onError?: (message: string) => void;
};

export type AsrSession = {
  stop: () => void;
  /** 测试/无 SpeechRecognition 时注入 */
  injectPartial?: (text: string) => void;
  injectFinal?: (text: string) => void;
};

export type AsrBackendKind = "webspeech" | "remote" | "remote-stub";

declare global {
  interface Window {
    webkitSpeechRecognition?: new () => SpeechRecognitionLike;
    SpeechRecognition?: new () => SpeechRecognitionLike;
    __AN_ASR_ACTIVE__?: AsrSession | null;
    /** E2E / 调试：webspeech | remote | remote-stub */
    __AN_DESKTOP_ASR_BACKEND__?: AsrBackendKind;
  }
}

type SpeechRecognitionLike = {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((ev: SpeechRecognitionEventLike) => void) | null;
  onerror: ((ev: { error?: string }) => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
};

type SpeechRecognitionEventLike = {
  resultIndex: number;
  results: ArrayLike<{ isFinal: boolean; 0: { transcript: string } }>;
};

export function getAsrBackend(): AsrBackendKind {
  if (typeof window !== "undefined") {
    const w = window.__AN_DESKTOP_ASR_BACKEND__;
    if (w === "webspeech" || w === "remote" || w === "remote-stub") return w;
  }
  const env = import.meta.env.VITE_DESKTOP_ASR_BACKEND;
  if (env === "remote" || env === "remote-stub" || env === "webspeech") return env;
  return "webspeech";
}

function recognitionCtor(): (new () => SpeechRecognitionLike) | null {
  if (typeof window === "undefined") return null;
  return window.SpeechRecognition ?? window.webkitSpeechRecognition ?? null;
}

function startWebSpeechAsr(handlers: AsrHandlers): AsrSession {
  const Ctor = recognitionCtor();
  let stopped = false;

  const harness: AsrSession = {
    stop: () => {
      stopped = true;
      if (typeof window !== "undefined" && window.__AN_ASR_ACTIVE__ === harness) {
        window.__AN_ASR_ACTIVE__ = null;
      }
    },
    injectPartial: (text: string) => {
      if (!stopped && text.trim()) handlers.onPartial?.(text.trim());
    },
    injectFinal: (text: string) => {
      if (!stopped && text.trim()) handlers.onFinal?.(text.trim());
    },
  };

  if (!Ctor) {
    if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = harness;
    return harness;
  }

  const rec = new Ctor();
  rec.continuous = true;
  rec.interimResults = true;
  rec.lang = "zh-CN";
  rec.onresult = (ev) => {
    if (stopped) return;
    let interim = "";
    let finalText = "";
    for (let i = ev.resultIndex; i < ev.results.length; i++) {
      const r = ev.results[i];
      const t = r[0]?.transcript ?? "";
      if (r.isFinal) finalText += t;
      else interim += t;
    }
    if (interim.trim()) handlers.onPartial?.(interim.trim());
    if (finalText.trim()) handlers.onFinal?.(finalText.trim());
  };
  rec.onerror = (ev) => {
    if (stopped) return;
    handlers.onError?.(ev.error ?? "speech_error");
  };
  rec.onend = () => {
    /* continuous 结束不自动重启，由 UI 控制 */
  };

  try {
    rec.start();
  } catch (e) {
    handlers.onError?.(e instanceof Error ? e.message : "asr_start_failed");
  }

  const session: AsrSession = {
    stop: () => {
      stopped = true;
      try {
        rec.abort();
      } catch {
        try {
          rec.stop();
        } catch {
          /* ignore */
        }
      }
      if (typeof window !== "undefined" && window.__AN_ASR_ACTIVE__ === session) {
        window.__AN_ASR_ACTIVE__ = null;
      }
    },
    injectPartial: harness.injectPartial,
    injectFinal: harness.injectFinal,
  };
  if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = session;
  return session;
}

export type StartAsrOptions = {
  stream?: MediaStream | null;
  /** 远端失败时是否回退 Web Speech（默认 true） */
  fallbackWebSpeech?: boolean;
  onBackend?: (kind: AsrBackendKind, detail?: string) => void;
};

/** 按旗标启动 ASR；remote 无配置/失败时回退 webspeech。 */
export async function startAsr(
  handlers: AsrHandlers,
  opts?: StartAsrOptions,
): Promise<AsrSession> {
  const kind = getAsrBackend();
  const fallback = opts?.fallbackWebSpeech !== false;

  if (kind === "remote-stub") {
    opts?.onBackend?.("remote-stub");
    return startRemoteStubAsr(handlers);
  }

  if (kind === "remote") {
    const live = await tryStartRemoteLiveAsr(handlers, { stream: opts?.stream });
    if (live) {
      opts?.onBackend?.("remote");
      return live;
    }
    if (fallback) {
      opts?.onBackend?.("webspeech", "remote_fallback");
      return startWebSpeechAsr(handlers);
    }
    opts?.onBackend?.("remote", "unavailable");
    const empty: AsrSession = {
      stop: () => {
        if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = null;
      },
      injectPartial: (t) => handlers.onPartial?.(t.trim()),
      injectFinal: (t) => handlers.onFinal?.(t.trim()),
    };
    if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = empty;
    return empty;
  }

  opts?.onBackend?.("webspeech");
  return startWebSpeechAsr(handlers);
}
