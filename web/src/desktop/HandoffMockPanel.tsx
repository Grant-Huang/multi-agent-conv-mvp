import { useState } from "react";
import {
  chooseConflict,
  resetHandoff,
  setVoiceShared,
  writeDeviceDraft,
  type HandoffState,
} from "./handoffMock";

export function HandoffMockPanel() {
  const [s, setS] = useState<HandoffState>(() => resetHandoff());

  const apply = (next: HandoffState) => setS(next);

  return (
    <div className="handoff-mock" data-testid="handoff-mock-panel">
      <h3>跨端续接（模拟）</h3>
      <dl className="evidence-dl">
        <dt>Task</dt>
        <dd data-testid="handoff-task">{s.taskId}</dd>
        <dt>Run</dt>
        <dd data-testid="handoff-run">{s.runId}</dd>
        <dt>阅读锚点</dt>
        <dd data-testid="handoff-anchor">{s.readMessageId}</dd>
        <dt>冲突</dt>
        <dd data-testid="handoff-conflict">{s.conflict ? "有" : "无"}</dd>
        <dt>选用</dt>
        <dd data-testid="handoff-chosen">{s.chosen ?? "—"}</dd>
        <dt>语音转写共享</dt>
        <dd data-testid="handoff-voice-shared">{s.voiceShared ? "共享中" : "未授权不共享"}</dd>
      </dl>
      <label className="settings-field">
        手机草稿
        <input
          data-testid="handoff-phone"
          value={s.phone.text}
          onChange={(e) => apply(writeDeviceDraft("phone", e.target.value))}
        />
      </label>
      <label className="settings-field">
        桌面草稿
        <input
          data-testid="handoff-desktop"
          value={s.desktop.text}
          onChange={(e) => apply(writeDeviceDraft("desktop", e.target.value))}
        />
      </label>
      <p className="muted">
        手机 v{s.phone.version} · 桌面 v{s.desktop.version}
      </p>
      <div className="settings-actions">
        <button type="button" className="btn small" data-testid="handoff-reset" onClick={() => apply(resetHandoff())}>
          重置
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-choose-phone"
          disabled={!s.conflict}
          onClick={() => apply(chooseConflict("phone"))}
        >
          采用手机稿
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-choose-desktop"
          disabled={!s.conflict}
          onClick={() => apply(chooseConflict("desktop"))}
        >
          采用桌面稿
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="handoff-voice-share"
          aria-pressed={s.voiceShared}
          onClick={() => apply(setVoiceShared(!s.voiceShared))}
        >
          {s.voiceShared ? "关闭转写共享" : "授权转写共享"}
        </button>
      </div>
    </div>
  );
}
