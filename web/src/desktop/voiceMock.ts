/** 语音 Mock（A5.3 / AD-VOICE / M6）；无真实媒体，仅可观察状态。 */

export type MicPermission = "prompt" | "granted" | "denied";
export type VoicePhase = "idle" | "connecting" | "listening" | "speaking" | "muted" | "offline";

export type VoiceSession = {
  taskId: string;
  permission: MicPermission;
  phase: VoicePhase;
  playing: boolean;
  partial: string;
  finalText: string;
  linkedTaskId: string | null;
  shareTranscript: boolean;
};

const KEY = "an.desktop.voiceMock.v1";

export function loadVoiceSession(): VoiceSession {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw) as VoiceSession;
  } catch {
    /* 忽略 */
  }
  return emptySession();
}

function emptySession(): VoiceSession {
  return {
    taskId: "TASK-2026-018",
    permission: "prompt",
    phase: "idle",
    playing: false,
    partial: "",
    finalText: "",
    linkedTaskId: null,
    shareTranscript: false,
  };
}

function save(s: VoiceSession): VoiceSession {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* 忽略 */
  }
  return s;
}

export function resetVoice(): VoiceSession {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
  return emptySession();
}

export function setPermission(p: MicPermission): VoiceSession {
  const s = loadVoiceSession();
  if (p === "denied") {
    return save({ ...s, permission: p, phase: "idle", playing: false, partial: "" });
  }
  return save({ ...s, permission: p });
}

export function startListening(): VoiceSession {
  const s = loadVoiceSession();
  if (s.permission === "denied") return s;
  if (s.permission === "prompt") {
    return save({ ...s, permission: "granted", phase: "connecting" });
  }
  return save({ ...s, phase: "listening", playing: false });
}

export function markConnected(): VoiceSession {
  const s = loadVoiceSession();
  if (s.phase !== "connecting") return s;
  return save({ ...s, phase: "listening" });
}

export function setPartial(text: string): VoiceSession {
  const s = loadVoiceSession();
  if (s.phase !== "listening" && s.phase !== "speaking") return s;
  return save({ ...s, partial: text });
}

/** 最终转写才进入工作判断；可纠正归属。 */
export function commitFinal(text: string, linkTask: boolean): VoiceSession {
  const s = loadVoiceSession();
  return save({
    ...s,
    partial: "",
    finalText: text,
    linkedTaskId: linkTask ? s.taskId : null,
    phase: "listening",
  });
}

export function startPlayback(): VoiceSession {
  const s = loadVoiceSession();
  return save({ ...s, playing: true, phase: "speaking" });
}

/** 插话：停止播报，保留已确认转写。 */
export function bargeIn(): VoiceSession {
  const s = loadVoiceSession();
  return save({ ...s, playing: false, phase: "listening" });
}

export function goOffline(): VoiceSession {
  const s = loadVoiceSession();
  return save({ ...s, phase: "offline", playing: false, partial: "" });
}

export function continueAsText(text: string): VoiceSession {
  const s = loadVoiceSession();
  return save({
    ...s,
    phase: s.permission === "denied" || s.phase === "offline" ? s.phase : "listening",
    finalText: text,
    partial: "",
  });
}

export function setShareTranscript(share: boolean): VoiceSession {
  const s = loadVoiceSession();
  return save({ ...s, shareTranscript: share });
}
