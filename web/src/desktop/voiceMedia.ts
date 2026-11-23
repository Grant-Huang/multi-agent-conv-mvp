/** 真机 mic 媒体模式与采集（T-75）；会话态仍走 /voice/* 服务权威。 */

export type VoiceMediaMode = "fixture" | "real";

declare global {
  interface Window {
    /** E2E / 调试覆盖；优先于 VITE_DESKTOP_VOICE_MEDIA */
    __AN_DESKTOP_VOICE_MEDIA__?: VoiceMediaMode;
  }
}

export function getVoiceMediaMode(): VoiceMediaMode {
  if (typeof window !== "undefined") {
    const w = window.__AN_DESKTOP_VOICE_MEDIA__;
    if (w === "real" || w === "fixture") return w;
  }
  const env = import.meta.env.VITE_DESKTOP_VOICE_MEDIA;
  return env === "real" ? "real" : "fixture";
}

export type MicRequestResult =
  | { ok: true; stream: MediaStream }
  | { ok: false; reason: "denied" | "unavailable" };

/** 用户手势下请求麦克风；不写服务权威。 */
export async function requestMicrophone(): Promise<MicRequestResult> {
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
    return { ok: false, reason: "unavailable" };
  }
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    return { ok: true, stream };
  } catch (e) {
    const name =
      e instanceof DOMException
        ? e.name
        : e && typeof e === "object" && "name" in e
          ? String((e as { name: string }).name)
          : "";
    if (name === "NotAllowedError" || name === "PermissionDeniedError" || name === "SecurityError") {
      return { ok: false, reason: "denied" };
    }
    return { ok: false, reason: "unavailable" };
  }
}

export function stopMediaStream(stream: MediaStream | null | undefined): void {
  if (!stream) return;
  for (const t of stream.getTracks()) {
    try {
      t.stop();
    } catch {
      /* ignore */
    }
  }
}
