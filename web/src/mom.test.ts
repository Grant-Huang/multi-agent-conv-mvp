import html from "../mom/index.html?raw";
import { describe, expect, it, vi } from "vitest";

// Exercise the actual standalone MOM page, including its command payloads and
// text-only rendering of untrusted evidence. No copy of the UI implementation.
describe("MOM human and agent board", () => {
  it("creates a scenario, renders evidence safely, switches views and approves exact plan", async () => {
    const calls: { path: string; body: Record<string, unknown> | undefined }[] = [];
    let created = false;
    const c = {
      case_id: "CASE-s1-ui", case_revision: 8, plan_revision: 2,
      projection_seq: 12, stage: "WAITING_HUMAN", scenario: "s1",
      constraints: ["Never bypass quality"], budget: { remaining: 20 },
      deadlines: {}, alerts: [], breaches: [], dag: {}, children: [], actions: {},
      evidence: { "E-1": { claim: "<img src=x onerror=alert(1)>" } },
      mock_ids: ["M22"], provenance: "mock", limitations: ["Simulation"],
    };
    const mock = vi.fn(async (path: string, init?: RequestInit) => {
      const body = init?.body ? JSON.parse(String(init.body)) as Record<string, unknown> : undefined;
      calls.push({ path, body });
      let response: unknown = c;
      if (path.endsWith("/scenarios/s1")) { created = true; response = { case_id: c.case_id }; }
      else if (path === "/api/mom/cases") response = { now: 514, cases: created ? [c] : [] };
      else if (path.includes("/events?")) response = [];
      else if (path.includes("/agent?")) response = { projection_seq: 12, constraints: c.constraints, next_cursor: 12 };
      return { ok: true, json: async () => response } as Response;
    });
    vi.stubGlobal("fetch", mock);
    const parsed = new DOMParser().parseFromString(html, "text/html");
    document.body.innerHTML = parsed.body.innerHTML;
    const script = parsed.querySelector("script")!.textContent!;
    // An anonymous function avoids polluting globals across Vitest suites.
    window.eval(`(function(){${script}\n})();`);
    await vi.waitFor(() => expect(mock).toHaveBeenCalled());
    (document.querySelector('[data-scenario="s1"]') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(document.getElementById("content")!.textContent).toContain("CASE-s1-ui"));
    expect(document.querySelector("#content img")).toBeNull();
    expect(document.getElementById("content")!.textContent).toContain("<img src=x onerror=alert(1)>");
    const approve = Array.from(document.querySelectorAll("button")).find(b => b.textContent === "批准")!;
    approve.click();
    await vi.waitFor(() => expect(calls.some(x => x.body?.command === "approve")).toBe(true));
    expect(calls.find(x => x.body?.command === "approve")!.body).toEqual({ command: "approve", scope: "production", actor: "supervisor", plan_revision: 2 });
    document.getElementById("agent")!.click();
    await vi.waitFor(() => expect(document.getElementById("content")!.textContent).toContain("next_cursor"));
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });
});
