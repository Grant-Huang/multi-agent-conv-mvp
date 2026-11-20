/** 跨端续接 Mock（A5.3 / AD-HANDOFF / M6）；双端草稿版本冲突可选。 */

export type DeviceId = "phone" | "desktop";

export type HandoffDraft = {
  device: DeviceId;
  text: string;
  version: number;
};

export type HandoffState = {
  taskId: string;
  conversationId: string;
  runId: string;
  readMessageId: string;
  phone: HandoffDraft;
  desktop: HandoffDraft;
  conflict: boolean;
  chosen: DeviceId | null;
  voiceShared: boolean;
};

const KEY = "an.desktop.handoffMock.v1";

export function loadHandoff(): HandoffState {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return JSON.parse(raw) as HandoffState;
  } catch {
    /* 忽略 */
  }
  return empty();
}

function empty(): HandoffState {
  return {
    taskId: "TASK-2026-018",
    conversationId: "conv-smt-03",
    runId: "run-smt-03-1",
    readMessageId: "msg-anchor-1",
    phone: { device: "phone", text: "", version: 0 },
    desktop: { device: "desktop", text: "", version: 0 },
    conflict: false,
    chosen: null,
    voiceShared: false,
  };
}

function save(s: HandoffState): HandoffState {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* 忽略 */
  }
  return s;
}

export function resetHandoff(): HandoffState {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
  return empty();
}

export function writeDeviceDraft(device: DeviceId, text: string): HandoffState {
  const s = loadHandoff();
  const cur = device === "phone" ? s.phone : s.desktop;
  const other = device === "phone" ? s.desktop : s.phone;
  const next: HandoffDraft = { device, text, version: cur.version + 1 };
  const conflict = other.version > 0 && other.text !== text && other.text.length > 0;
  return save({
    ...s,
    phone: device === "phone" ? next : s.phone,
    desktop: device === "desktop" ? next : s.desktop,
    conflict,
    chosen: conflict ? null : s.chosen,
  });
}

export function chooseConflict(device: DeviceId): HandoffState {
  const s = loadHandoff();
  const keep = device === "phone" ? s.phone : s.desktop;
  return save({
    ...s,
    phone: { ...keep, device: "phone" },
    desktop: { ...keep, device: "desktop" },
    conflict: false,
    chosen: device,
  });
}

export function setVoiceShared(shared: boolean): HandoffState {
  const s = loadHandoff();
  return save({ ...s, voiceShared: shared });
}
