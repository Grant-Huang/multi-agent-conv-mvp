/**
 * 可选远端流式 ASR（T-76）。
 * - remote-stub：本地回放 NDJSON 事件，不上送音频、不需密钥（CI / 合同落地）
 * - remote：连 VITE_DESKTOP_ASR_URL；无钥/失败则返回 null 由上层回退 Web Speech
 * 不落盘原始录音。
 */

import type { AsrHandlers, AsrSession } from "./voiceAsr";

export type RemoteAsrEvent = {
  t_ms: number;
  kind: "partial" | "final" | "error";
  text: string;
};

/** 质量试点用固定回放（可辨远端路径，非真钥质量） */
export const REMOTE_ASR_STUB_REPLAY: RemoteAsrEvent[] = [
  { t_ms: 40, kind: "partial", text: "远端…SMT" },
  { t_ms: 110, kind: "partial", text: "远端试点 SMT-03 振动" },
];

export type RemoteAsrConfig = {
  url: string;
  token: string;
  /** 默认 false：只发会话信令，不传 PCM；真钥试点可开 */
  sendAudio: boolean;
};

export function readRemoteAsrConfig(): RemoteAsrConfig | null {
  const url =
    (typeof window !== "undefined" &&
      (window as unknown as { __AN_DESKTOP_ASR_URL__?: string }).__AN_DESKTOP_ASR_URL__) ||
    import.meta.env.VITE_DESKTOP_ASR_URL ||
    "";
  const token =
    (typeof window !== "undefined" &&
      (window as unknown as { __AN_DESKTOP_ASR_TOKEN__?: string }).__AN_DESKTOP_ASR_TOKEN__) ||
    import.meta.env.VITE_DESKTOP_ASR_TOKEN ||
    "";
  const sendAudio =
    import.meta.env.VITE_DESKTOP_ASR_SEND_AUDIO === "1" ||
    (typeof window !== "undefined" &&
      (window as unknown as { __AN_DESKTOP_ASR_SEND_AUDIO__?: string }).__AN_DESKTOP_ASR_SEND_AUDIO__ ===
        "1");
  if (!String(url).trim()) return null;
  return { url: String(url).trim(), token: String(token), sendAudio };
}

export function startRemoteStubAsr(
  handlers: AsrHandlers,
  replay: RemoteAsrEvent[] = REMOTE_ASR_STUB_REPLAY,
): AsrSession {
  let stopped = false;
  const timers: number[] = [];

  for (const ev of replay) {
    const id = window.setTimeout(() => {
      if (stopped) return;
      if (ev.kind === "partial") handlers.onPartial?.(ev.text);
      else if (ev.kind === "final") handlers.onFinal?.(ev.text);
      else handlers.onError?.(ev.text);
    }, ev.t_ms);
    timers.push(id);
  }

  const session: AsrSession = {
    stop: () => {
      stopped = true;
      for (const id of timers) window.clearTimeout(id);
      if (typeof window !== "undefined" && window.__AN_ASR_ACTIVE__ === session) {
        window.__AN_ASR_ACTIVE__ = null;
      }
    },
    injectPartial: (text) => {
      if (!stopped && text.trim()) handlers.onPartial?.(text.trim());
    },
    injectFinal: (text) => {
      if (!stopped && text.trim()) handlers.onFinal?.(text.trim());
    },
  };
  if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = session;
  return session;
}

/**
 * 远端 live：WebSocket 优先；否则 HTTP POST NDJSON 拉流。
 * 协议（占位，厂商可换）：服务端推送行 `{"type":"partial"|"final"|"error","text":"..."}`。
 * 客户端默认只发 start/stop JSON，不传音频（sendAudio 才允许后续扩展）。
 * 失败返回 null，调用方回退。
 */
export async function tryStartRemoteLiveAsr(
  handlers: AsrHandlers,
  opts?: { stream?: MediaStream | null },
): Promise<AsrSession | null> {
  const cfg = readRemoteAsrConfig();
  if (!cfg) {
    handlers.onError?.("remote_asr_unconfigured");
    return null;
  }

  const sessionId = `asr-${Date.now().toString(36)}`;
  let stopped = false;
  let ws: WebSocket | null = null;
  let abort: AbortController | null = null;

  const applyLine = (raw: string) => {
    if (stopped || !raw.trim()) return;
    try {
      const msg = JSON.parse(raw) as { type?: string; text?: string };
      const t = (msg.text ?? "").trim();
      if (msg.type === "partial" && t) handlers.onPartial?.(t);
      else if (msg.type === "final" && t) handlers.onFinal?.(t);
      else if (msg.type === "error") handlers.onError?.(t || "remote_asr_error");
    } catch {
      /* ignore non-json */
    }
  };

  const stopAll = () => {
    stopped = true;
    try {
      ws?.close();
    } catch {
      /* ignore */
    }
    ws = null;
    abort?.abort();
    abort = null;
    if (typeof window !== "undefined" && window.__AN_ASR_ACTIVE__ === session) {
      window.__AN_ASR_ACTIVE__ = null;
    }
  };

  const session: AsrSession = {
    stop: stopAll,
    injectPartial: (text) => {
      if (!stopped && text.trim()) handlers.onPartial?.(text.trim());
    },
    injectFinal: (text) => {
      if (!stopped && text.trim()) handlers.onFinal?.(text.trim());
    },
  };

  const startPayload = JSON.stringify({
    type: "start",
    session_id: sessionId,
    destination_id: "desktop-asr-stream",
    send_audio: cfg.sendAudio && !!opts?.stream,
    lang: "zh-CN",
  });

  try {
    if (cfg.url.startsWith("ws://") || cfg.url.startsWith("wss://")) {
      await new Promise<void>((resolve, reject) => {
        ws = new WebSocket(cfg.url);
        const t = window.setTimeout(() => reject(new Error("remote_asr_ws_timeout")), 4000);
        ws.onopen = () => {
          window.clearTimeout(t);
          try {
            if (cfg.token) {
              ws?.send(JSON.stringify({ type: "auth", token: cfg.token }));
            }
            ws?.send(startPayload);
            resolve();
          } catch (e) {
            reject(e);
          }
        };
        ws.onerror = () => {
          window.clearTimeout(t);
          reject(new Error("remote_asr_ws_error"));
        };
        ws.onmessage = (ev) => applyLine(String(ev.data));
        ws.onclose = () => {
          if (!stopped) handlers.onError?.("remote_asr_ws_closed");
        };
      });
    } else {
      abort = new AbortController();
      const headers: Record<string, string> = {
        "Content-Type": "application/json",
        Accept: "application/x-ndjson, application/json, text/plain",
      };
      if (cfg.token) headers.Authorization = `Bearer ${cfg.token}`;
      const res = await fetch(cfg.url, {
        method: "POST",
        headers,
        body: startPayload,
        signal: abort.signal,
      });
      if (!res.ok) {
        handlers.onError?.(`remote_asr_http_${res.status}`);
        stopAll();
        return null;
      }
      if (res.body) {
        const reader = res.body.getReader();
        const dec = new TextDecoder();
        let buf = "";
        void (async () => {
          try {
            while (!stopped) {
              const { done, value } = await reader.read();
              if (done) break;
              buf += dec.decode(value, { stream: true });
              const lines = buf.split("\n");
              buf = lines.pop() ?? "";
              for (const line of lines) applyLine(line);
            }
          } catch {
            if (!stopped) handlers.onError?.("remote_asr_stream_error");
          }
        })();
      }
    }
  } catch (e) {
    handlers.onError?.(e instanceof Error ? e.message : "remote_asr_connect_failed");
    stopAll();
    return null;
  }

  // 明确：默认不把 MediaStream PCM 上传；sendAudio 仅占位，真钥试点另开开关与证据
  void opts;
  void cfg.sendAudio;

  if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = session;
  return session;
}
