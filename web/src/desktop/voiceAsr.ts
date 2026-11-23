/**
 * ASR 适配器：Web Speech 为主；Harness 供 Playwright 在 fake device 下注入转写（T-75）。
 * 仅产出文本；服务权威仍由调用方 POST /voice/partial|finalize。
 */

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

declare global {
  interface Window {
    webkitSpeechRecognition?: new () => SpeechRecognitionLike;
    SpeechRecognition?: new () => SpeechRecognitionLike;
    __AN_ASR_ACTIVE__?: AsrSession | null;
  }
}

function recognitionCtor(): (new () => SpeechRecognitionLike) | null {
  if (typeof window === "undefined") return null;
  return window.SpeechRecognition ?? window.webkitSpeechRecognition ?? null;
}

/** 启动 ASR；无浏览器 SpeechRecognition 时退化为可注入 Harness。 */
export function startAsr(handlers: AsrHandlers): AsrSession {
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
