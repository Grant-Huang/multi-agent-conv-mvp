/** 个人助理身份与设置（A4 / M3 Mock）；本地版本号用于冲突演示。 */
export type AssistantProfile = {
  name: string;
  avatar: string;
  role: string;
  soul: string;
  taste: string;
  memoryPrinciple: string;
  proactivity: string;
  version: number;
};

export type MemoryEntry = {
  id: string;
  title: string;
  source: string;
  body: string;
  expires: string;
};

const PROFILE_KEY = "an.desktop.assistantProfile.v1";

export const DEFAULT_PROFILE: AssistantProfile = {
  name: "我的助理",
  avatar: "◇",
  role: "日常工作伙伴",
  soul: "简洁、可核对、不替你做未授权决定",
  taste: "优先事实与可执行下一步",
  memoryPrinciple: "仅保留你确认过的长期偏好",
  proactivity: "克制：重要事项可提醒，不抢焦点",
  version: 1,
};

export const MEMORY_SEED: MemoryEntry[] = [
  {
    id: "mem-1",
    title: "质量报告偏好摘要格式",
    source: "用户确认 · 2026-11-18",
    body: "先给结论，再列证据条目与待核实项。",
    expires: "2027-11-18",
  },
  {
    id: "mem-2",
    title: "免打扰时段",
    source: "设置 · 通知与主动性",
    body: "工作日 22:00–08:00 不推普通建议。",
    expires: "长期",
  },
];

export function loadProfile(): AssistantProfile {
  try {
    const raw = localStorage.getItem(PROFILE_KEY);
    if (!raw) return { ...DEFAULT_PROFILE };
    const p = JSON.parse(raw) as Partial<AssistantProfile>;
    return { ...DEFAULT_PROFILE, ...p, version: typeof p.version === "number" ? p.version : 1 };
  } catch {
    return { ...DEFAULT_PROFILE };
  }
}

export function saveProfile(next: AssistantProfile, expectedVersion: number): { ok: true; profile: AssistantProfile } | { ok: false; current: AssistantProfile; reason: string } {
  const current = loadProfile();
  if (current.version !== expectedVersion) {
    return { ok: false, current, reason: "版本冲突：设置已被更新，请基于当前值重新编辑" };
  }
  const saved = { ...next, version: expectedVersion + 1 };
  try {
    localStorage.setItem(PROFILE_KEY, JSON.stringify(saved));
  } catch {
    return { ok: false, current, reason: "保存失败：本地存储不可用" };
  }
  return { ok: true, profile: saved };
}

/** 测试夹具：抬高已存版本，使下次保存冲突 */
export function bumpProfileVersionForConflict(): void {
  const p = loadProfile();
  try {
    localStorage.setItem(PROFILE_KEY, JSON.stringify({ ...p, version: p.version + 1 }));
  } catch {
    /* 忽略 */
  }
}

export const SETTINGS_CATEGORIES = [
  { id: "assistant", title: "我的 AI 助理" },
  { id: "desktop", title: "工作与桌面" },
  { id: "notify", title: "通知与主动性" },
  { id: "device", title: "设备与语音" },
  { id: "privacy", title: "隐私与权限" },
  { id: "connect", title: "连接与能力" },
] as const;

export type SettingsCategoryId = (typeof SETTINGS_CATEGORIES)[number]["id"];
