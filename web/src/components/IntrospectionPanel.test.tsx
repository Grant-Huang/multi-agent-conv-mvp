// 业务内省页：接口缺失（旧后端）时不能只显示生硬的 "Error: HTTP 404"。
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { IntrospectionPanel } from "./IntrospectionPanel";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("IntrospectionPanel", () => {
  it("findings 404 时提示需重新编译后端，而不是裸 Error: HTTP 404", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: false,
        status: 404,
        json: async () => ({}),
      })),
    );
    render(<IntrospectionPanel />);
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    const text = screen.getByRole("alert").textContent ?? "";
    expect(text).not.toMatch(/^Error: HTTP 404$/);
    expect(text).toMatch(/内省接口不可用|重新编译|重启/);
  });
});
