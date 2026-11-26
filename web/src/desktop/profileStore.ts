/** 个人助理身份与设置（A4 / T-70 B3 preferences；T-71 记忆列表接服务）。 */
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
  /** UXFIX-T（#353）：类型（fact/preference/style）与确认状态（服务端已序列化） */
  kind: string;
  confirmed: boolean;
};

/** UXFIX-T：记忆类型用户语言（CH-G2e） */
export function memoryKindText(kind: string): string {
  switch (kind) {
    case "fact":
      return "事实";
    case "preference":
      return "偏好";
    case "style":
      return "风格";
    default:
      return kind;
  }
}

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

/** T-71：从服务拉取记忆列表 */
export async function fetchMemories(): Promise<MemoryEntry[]> {
  const r = await api.desktopMemoriesList();
  return r.items.map((m) => ({
    id: m.id,
    title: m.title,
    source: m.source,
    body: m.body,
    expires: m.expires,
    kind: m.kind,
    confirmed: m.confirmed,
  }));
}

export async function fetchMemory(id: string): Promise<MemoryEntry> {
  const m = await api.desktopMemoryGet(id);
  return {
    id: m.id,
    title: m.title,
    source: m.source,
    body: m.body,
    expires: m.expires,
    kind: m.kind,
    confirmed: m.confirmed,
  };
}

/** T-72：纠错正文/标题/有效期 */
export async function correctMemory(
  id: string,
  patch: { title?: string; body?: string; expires?: string },
): Promise<MemoryEntry> {
  const m = await api.desktopMemoryPatch(id, patch);
  return {
    id: m.id,
    title: m.title,
    source: m.source,
    body: m.body,
    expires: m.expires,
    kind: m.kind,
    confirmed: m.confirmed,
  };
}

/** T-72：撤回（默认列表隐藏） */
export async function withdrawMemory(id: string): Promise<void> {
  await api.desktopMemoryDelete(id);
}

export async function resetMemoriesFixture(): Promise<void> {
  await api.desktopMemoriesReset();
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
