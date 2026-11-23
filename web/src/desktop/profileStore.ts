/** 个人助理身份与设置（A4 / T-70 接 B3 preferences）。记忆列表仍为本地种子。 */
import { api, type DesktopPreferencesDto } from "../api";

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

export function fromPreferencesDto(p: DesktopPreferencesDto): AssistantProfile {
  return {
    name: p.name,
    avatar: p.avatar,
    role: p.role,
    soul: p.soul,
    taste: p.taste,
    memoryPrinciple: p.memory_principle,
    proactivity: p.proactivity,
    version: p.version,
  };
}

/** 从 B3 拉取当前配置（无行时服务端返回默认 version=1） */
export async function fetchProfile(): Promise<AssistantProfile> {
  const p = await api.desktopPreferencesGet();
  return fromPreferencesDto(p);
}

/** @deprecated 同步本地读已移除；请用 fetchProfile。保留默认值供首屏占位。 */
export function loadProfile(): AssistantProfile {
  return { ...DEFAULT_PROFILE };
}

export async function saveProfile(
  next: AssistantProfile,
  expectedVersion: number,
): Promise<
  { ok: true; profile: AssistantProfile } | { ok: false; current: AssistantProfile; reason: string }
> {
  const r = await api.desktopPreferencesPatch({
    expected_version: expectedVersion,
    name: next.name,
    avatar: next.avatar,
    role: next.role,
    soul: next.soul,
    taste: next.taste,
    memory_principle: next.memoryPrinciple,
    proactivity: next.proactivity,
  });
  if (!r.ok) {
    return {
      ok: false,
      current: r.current ? fromPreferencesDto(r.current) : { ...DEFAULT_PROFILE },
      reason:
        r.code === "version_conflict"
          ? "版本冲突：设置已被更新，请基于当前值重新编辑"
          : r.message,
    };
  }
  return { ok: true, profile: fromPreferencesDto(r.body) };
}

/** 测试夹具：服务端升版，使下次带旧 expected_version 的保存冲突 */
export async function bumpProfileVersionForConflict(): Promise<void> {
  const cur = await api.desktopPreferencesGet();
  const r = await api.desktopPreferencesPatch({ expected_version: cur.version });
  if (!r.ok) {
    throw new Error(r.message);
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
