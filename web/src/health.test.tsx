// 连接状态圆点：绿 = 已连通，灰 = 已配置但未连通，红 = 不可用；灰、红的原因写在悬停提示里
// T-88：诊断行默认折叠到「连接详情」，用例先展开再断言（断言本体不变，检查强度不降）
import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Health } from "./api";
import { HealthBox } from "./App";

type Link = { status: "connected" | "configured" | "unavailable"; reason: string };

function agent(id: string, link: Link) {
  return {
    id,
    kind: "acp",
    agent_type: id,
    location: "本机",
    max_concurrent: 1,
    role: "coding",
    name: id,
    description: "",
    tools: [],
    enabled: link.status !== "unavailable",
    disabled_reason: link.status === "unavailable" ? link.reason : null,
    link,
  };
}

const health = {
  llm: { ok: true, base_url: "", model: "", server_models: [], error: null, structured_mode: "json_schema" },
  backends: [],
  agents: [
    agent("green-agent", { status: "connected", reason: "已连通" }),
    agent("grey-agent", { status: "configured", reason: "未连通：握手超时" }),
    agent("red-agent", { status: "unavailable", reason: "找不到命令 `x`" }),
  ],
} as unknown as Health;

function rowOf(container: HTMLElement, name: string): HTMLElement {
  const row = [...container.querySelectorAll<HTMLElement>(".agent-row")].find((r) => r.textContent?.includes(name));
  if (!row) throw new Error(`没有找到 ${name} 的行`);
  return row;
}

describe("agent 连接状态圆点", () => {
  it("绿灯=已连通，灰灯=已配置未连通，红灯=不可用", () => {
    const view = render(<HealthBox health={health} />);
    // 诊断行折叠在「连接详情」里，展开后断言
    fireEvent.click(view.getByRole("button", { name: /连接详情/ }));
    expect(rowOf(view.container, "green-agent").querySelector(".dot")!.className).toContain("ok");
    expect(rowOf(view.container, "grey-agent").querySelector(".dot")!.className).toContain("idle");
    expect(rowOf(view.container, "red-agent").querySelector(".dot")!.className).toContain("bad");
  });

  it("灰灯和红灯的原因写在悬停提示里", () => {
    const view = render(<HealthBox health={health} />);
    fireEvent.click(view.getByRole("button", { name: /连接详情/ }));
    expect(rowOf(view.container, "grey-agent").title).toBe("未连通：握手超时");
    expect(rowOf(view.container, "red-agent").title).toBe("找不到命令 `x`");
  });
});
