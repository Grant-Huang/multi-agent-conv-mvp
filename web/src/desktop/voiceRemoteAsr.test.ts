import { afterEach, describe, expect, it, vi } from "vitest";
import { REMOTE_ASR_STUB_REPLAY, startRemoteStubAsr } from "./voiceRemoteAsr";

describe("remote ASR stub", () => {
  afterEach(() => {
    vi.useRealTimers();
    if (typeof window !== "undefined") window.__AN_ASR_ACTIVE__ = null;
  });

  it("replays partial events without network", () => {
    vi.useFakeTimers();
    const partials: string[] = [];
    const session = startRemoteStubAsr({
      onPartial: (t) => partials.push(t),
    });
    expect(window.__AN_ASR_ACTIVE__).toBe(session);
    vi.advanceTimersByTime(REMOTE_ASR_STUB_REPLAY[REMOTE_ASR_STUB_REPLAY.length - 1]!.t_ms);
    expect(partials.some((t) => t.includes("SMT"))).toBe(true);
    session.stop();
    expect(window.__AN_ASR_ACTIVE__).toBeNull();
  });
});
