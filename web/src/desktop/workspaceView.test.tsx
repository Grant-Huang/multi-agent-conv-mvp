// UXFIX-B（#305，INTENT-003，CH-U5）：测试控件只在测试模式渲染（服务端告知）。
// 生产模式无法经界面打开工作区（无夹具任务），组件级测试覆盖三个面板的门控；
// 测试模式开启路径与现有 E2E（harness，test_fixtures=true）一致。
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceView } from "./WorkspaceView";
import { SettingsPanel } from "./SettingsPanel";
import { EvidencePanel } from "./EvidencePanel";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  localStorage.clear();
  sessionStorage.clear();
});

/** 助理设置异步读 profile（fetch），统一 stub 成功响应 */
function stubProfileFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({
        name: "我的助理",
        avatar: "",
        role: "",
        soul: "",
        taste: "",
        memory_principle: "",
        proactivity: "",
        version: 1,
      }),
    })),
  );
}

describe("WorkspaceView 测试控件门控", () => {
  it("测试模式关闭时不渲染模拟面板按钮", () => {
    const { container } = render(<WorkspaceView taskId="TASK-2026-018" onBack={() => {}} />);
    expect(container.querySelector('[data-testid="workspace-mock-toggles"]')).toBeNull();
    expect(container.textContent).not.toContain("动作审批模拟");
    expect(container.textContent).not.toContain("受理模拟");
    // 输入区仍在（对话可用性不受影响）
    expect(container.querySelector('[data-testid="workspace-input"]')).not.toBeNull();
  });

  it("测试模式开启时渲染模拟面板按钮（现有 E2E 路径不变）", () => {
    const { container } = render(<WorkspaceView taskId="TASK-2026-018" onBack={() => {}} testFixtures={true} />);
    expect(container.querySelector('[data-testid="workspace-mock-toggles"]')).not.toBeNull();
    expect(container.textContent).toContain("动作审批模拟");
  });
});

describe("EvidencePanel 测试控件门控", () => {
  const base = {
    taskId: "TASK-2026-018",
    layer: "list" as const,
    evidenceId: null,
    onLayer: () => {},
    onClose: () => {},
  };

  it("测试模式关闭时不渲染身份切换与空列表夹具", () => {
    const { container } = render(<EvidencePanel {...base} />);
    expect(container.querySelector('[data-testid="evidence-acl-bar"]')).toBeNull();
    expect(container.querySelector('[data-testid="evidence-demo-empty"]')).toBeNull();
  });

  it("测试模式开启时渲染身份切换（现有 E2E 路径不变）", () => {
    const { container } = render(<EvidencePanel {...base} testFixtures />);
    expect(container.querySelector('[data-testid="evidence-acl-bar"]')).not.toBeNull();
    expect(container.querySelector('[data-testid="evidence-principal-alice"]')).not.toBeNull();
  });
});

describe("SettingsPanel 测试控件门控", () => {
  const base = {
    layer: "assistant" as const,
    categoryId: "assistant" as const,
    memoryId: null,
    search: "",
    onNavigate: () => {},
    onClose: () => {},
    onProfileSaved: () => {},
  };

  it("测试模式关闭时不渲染注入冲突（测试）", () => {
    stubProfileFetch();
    const { container } = render(<SettingsPanel {...base} />);
    expect(container.querySelector('[data-testid="profile-inject-conflict"]')).toBeNull();
  });

  it("测试模式开启时渲染注入冲突（测试）（现有 E2E 路径不变）", async () => {
    stubProfileFetch();
    const { container } = render(<SettingsPanel {...base} testFixtures />);
    // profile 异步加载后表单才渲染注入按钮
    await waitFor(() => expect(container.querySelector('[data-testid="profile-inject-conflict"]')).not.toBeNull());
  });
});
