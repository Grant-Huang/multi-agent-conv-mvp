// UXFIX-N（#322，INTENT-021）：“稍后”只影响浮层（本机留存、到点恢复），不动服务端记录。
import { beforeEach, describe, expect, it } from "vitest";
import { filterSnoozed, snoozeAttention } from "./attentionStore";

beforeEach(() => {
  sessionStorage.clear();
});

describe("filterSnoozed", () => {
  it("稍后中的提醒被过滤，其余保留", () => {
    snoozeAttention("a", 5);
    const items = [
      { id: "a", title: "稍后中的" },
      { id: "b", title: "正常显示" },
    ];
    const out = filterSnoozed(items);
    expect(out.map((x) => x.id)).toEqual(["b"]);
  });

  it("到期后提醒恢复显示（顺手清理过期记录）", () => {
    snoozeAttention("a", 5);
    // 直接把到期时间改到过去，模拟时间流逝
    const raw = sessionStorage.getItem("an.desktop.attn.snooze.v1");
    const all = JSON.parse(raw!) as Record<string, number>;
    all["a"] = Date.now() - 1;
    sessionStorage.setItem("an.desktop.attn.snooze.v1", JSON.stringify(all));
    const out = filterSnoozed([{ id: "a", title: "已到期" }]);
    expect(out.map((x) => x.id)).toEqual(["a"]);
  });
});
