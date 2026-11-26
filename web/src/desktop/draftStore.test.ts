// UXFIX-D（#302，INTENT-2026-008/011/013）草稿存储约束的单元级 Red→Green：
// 1) 快照早于本地输入的远端草稿不得无提示覆盖本地（跨会话版本号不可比）；
// 2) 首次推送必带 expected_version（不得无前置条件覆盖其他会话的服务端草稿）；
// 3) 409 冲突保留本地文字并提示，不得静默改写；
// 4) "继续当前任务"与"独立话题"草稿分存（键带作用域），切换先存后取。
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  __resetDraftStoreForTest,
  clearDraftAfterSend,
  draftStorageKey,
  loadDraft,
  loadDraftRemote,
  onDraftConflict,
  saveDraft,
  scheduleDraftSync,
} from "./draftStore";
import { api } from "../api";

vi.mock("../api", () => ({
  api: {
    desktopDraftGet: vi.fn(),
    desktopDraftSave: vi.fn(),
    desktopDraftClearAfterSend: vi.fn(),
  },
}));

const KEY = draftStorageKey("task", "TASK-2026-018");

function serverDraft(text: string, version: number) {
  return { draft: { text, version, scope: "continue", continue_task_id: null, updated_at: "" } };
}

beforeEach(() => {
  vi.mocked(api.desktopDraftGet).mockReset();
  vi.mocked(api.desktopDraftSave).mockReset();
  vi.mocked(api.desktopDraftClearAfterSend).mockReset();
  localStorage.clear();
  sessionStorage.clear();
  __resetDraftStoreForTest();
});

describe("远端草稿不覆盖更新的本地输入（INTENT-008）", () => {
  it("GET 快照不含本地未同步的输入时，保留本地并报告冲突", async () => {
    // 第一轮：服务端旧草稿 X v7，本地为空 → 采纳（跨设备恢复，现行为）
    vi.mocked(api.desktopDraftGet).mockResolvedValue(serverDraft("其他会话的旧草稿", 7));
    const first = await loadDraftRemote(KEY);
    expect(first.draft.text).toBe("其他会话的旧草稿");

    // 用户在本会话输入新内容（尚未同步到服务端）
    saveDraft(KEY, { text: "本次的新调查内容" });

    // 第二轮 GET 返回的仍是旧快照（请求发出后另一会话/延迟响应的场景）
    const conflicts: string[] = [];
    const off = onDraftConflict(KEY, (c) => conflicts.push(c.serverText));
    const second = await loadDraftRemote(KEY);

    // 断言：本地新输入保留；冲突有提示（不得无提示丢失任何一方）
    expect(loadDraft(KEY).text).toBe("本次的新调查内容");
    expect(second.draft.text).toBe("本次的新调查内容");
    expect(second.conflict).not.toBeNull();
    expect(second.conflict?.serverText).toBe("其他会话的旧草稿");
    expect(conflicts).toContain("其他会话的旧草稿");
    off();
  });

  it("已确认同步后再收到相同内容不误报冲突", async () => {
    vi.mocked(api.desktopDraftGet).mockResolvedValue(serverDraft("A", 3));
    await loadDraftRemote(KEY); // 采纳 A v3
    vi.mocked(api.desktopDraftGet).mockResolvedValue(serverDraft("A", 3));
    const again = await loadDraftRemote(KEY);
    expect(again.draft.text).toBe("A");
    expect(again.conflict).toBeNull();
  });
});

describe("推送必带乐观版本（INTENT-008：不得无 expected_version 覆盖他人草稿）", () => {
  it("未取得服务端版本前先读取再推送，PUT 带 expected_version", async () => {
    vi.mocked(api.desktopDraftGet).mockResolvedValue(serverDraft("", 5));
    vi.mocked(api.desktopDraftSave).mockResolvedValue(serverDraft("待推送内容", 6));
    saveDraft(KEY, { text: "待推送内容" });
    scheduleDraftSync(KEY, 0);
    await vi.waitFor(() => expect(api.desktopDraftSave).toHaveBeenCalled());
    const body = vi.mocked(api.desktopDraftSave).mock.calls[0]?.[2];
    expect(body?.text).toBe("待推送内容");
    expect(body?.expected_version).toBe(5);
  });
});

describe("409 冲突保留本地并提示（INTENT-008）", () => {
  it("推送冲突时不改写本地文字，并发出冲突提示", async () => {
    vi.mocked(api.desktopDraftGet)
      .mockResolvedValueOnce(serverDraft("", 4)) // 首次推送前取得版本
      .mockResolvedValueOnce(serverDraft("其他设备的文字", 6)); // 409 后回读服务端
    vi.mocked(api.desktopDraftSave)
      .mockResolvedValueOnce(serverDraft("", 5)) // 首次空文本推送成功，取得版本
      .mockRejectedValueOnce(
        Object.assign(new Error("conflict"), {
          response: { status: 409, body: { draft: { text: "其他设备的文字", version: 6 } } },
        }),
      );
    scheduleDraftSync(KEY, 0);
    await vi.waitFor(() => expect(api.desktopDraftSave).toHaveBeenCalled());

    saveDraft(KEY, { text: "本机的文字" });
    const conflicts: string[] = [];
    const off = onDraftConflict(KEY, (c) => conflicts.push(c.serverText));
    scheduleDraftSync(KEY, 0);
    await vi.waitFor(() => expect(api.desktopDraftSave).toHaveBeenCalledTimes(2));
    // 等待冲突处理完成
    await new Promise((r) => setTimeout(r, 20));

    expect(loadDraft(KEY).text).toBe("本机的文字");
    expect(conflicts).toContain("其他设备的文字");
    off();
  });
});

describe("草稿键带作用域（INTENT-013）", () => {
  it("继续任务与独立话题分存，互不覆盖", () => {
    const baseKey = draftStorageKey("task", "TASK-2026-018");
    const indepKey = draftStorageKey("task", "TASK-2026-018", "independent");
    expect(indepKey).not.toBe(baseKey);
    saveDraft(baseKey, { text: "任务草稿" });
    saveDraft(indepKey, { text: "临时提问" });
    expect(loadDraft(baseKey).text).toBe("任务草稿");
    expect(loadDraft(indepKey).text).toBe("临时提问");
  });

  it("独立话题作用域的服务端同步使用带后缀的 scope_id", async () => {
    vi.mocked(api.desktopDraftGet).mockResolvedValue(serverDraft("", 0));
    const indepKey = draftStorageKey("task", "TASK-2026-018", "independent");
    await loadDraftRemote(indepKey);
    expect(vi.mocked(api.desktopDraftGet).mock.calls[0]?.[1]).toBe("TASK-2026-018@independent");
  });

  it("独立话题键的清空（发送后）只清该作用域", () => {
    const baseKey = draftStorageKey("task", "TASK-2026-018");
    const indepKey = draftStorageKey("task", "TASK-2026-018", "independent");
    saveDraft(baseKey, { text: "任务草稿" });
    saveDraft(indepKey, { text: "临时提问" });
    clearDraftAfterSend(indepKey, loadDraft(indepKey).version);
    expect(loadDraft(baseKey).text).toBe("任务草稿");
    expect(loadDraft(indepKey).text).toBe("");
  });
});
