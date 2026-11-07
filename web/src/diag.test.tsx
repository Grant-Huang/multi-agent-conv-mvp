// 诊断页的槽位行：只显示占用和生成状态；llama 的 id_task 不是任务数，不应显示为"已处理 N 个任务"
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SlotRow } from "./components/DiagnosticsPanel";

describe("诊断页槽位行", () => {
  it("正在生成的槽位显示占用者和生成状态，不显示 id_task 的累计数", () => {
    const { container } = render(
      <SlotRow slot={{ id: 2, class: "background", holder: "reviewer", held_ms: 1200, server_busy: true, tasks: 138703 }} />,
    );
    const text = container.textContent ?? "";
    expect(text).toContain("槽位 2（background）");
    expect(text).toContain("占用：reviewer");
    expect(text).toContain("正在生成");
    expect(text).not.toContain("已处理");
    expect(text).not.toContain("138703");
  });
});
