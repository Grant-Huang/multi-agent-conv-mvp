// UXFIX-L（#315，INTENT-2026-018）：普通模式进度行不暴露原始参数；开发者模式保留原文。
import { describe, expect, it } from "vitest";
import { userTaskLine } from "./taskLineText";

describe("userTaskLine", () => {
  it("普通模式：write_file 参数转为“写入文件 + 路径”", () => {
    const line = '第 1/6 步：调用工具 write_file，参数 {"path":"reports/verification.md","content":"# 报告"}';
    expect(userTaskLine(line, false)).toBe("第 1/6 步：写入文件 reports/verification.md");
  });

  it("普通模式：未知工具隐藏参数", () => {
    const line = '第 2/6 步：调用工具 mcp_lookup，参数 {"q":"x"}';
    expect(userTaskLine(line, false)).toBe("第 2/6 步：执行 mcp_lookup");
  });

  it("普通模式：非工具行不受影响", () => {
    expect(userTaskLine("正在收集证据", false)).toBe("正在收集证据");
  });

  it("开发者模式：保留原始行", () => {
    const line = '第 1/6 步：调用工具 write_file，参数 {"path":"a.md"}';
    expect(userTaskLine(line, true)).toBe(line);
  });
});
