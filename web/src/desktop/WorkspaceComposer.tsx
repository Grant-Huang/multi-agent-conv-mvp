import { useEffect, useRef, useState } from "react";
import {
  appendTaskLinkAudit,
  clearDraftAfterSend,
  draftStorageKey,
  listTaskLinkAudits,
  loadDraft,
  saveDraft,
  type DraftScope,
} from "./draftStore";
import { ScopeControls } from "./ScopeControls";

type Props = {
  taskId: string;
  delayMs?: number;
};

const LATE_PREFIX = "an.desktop.late.v1:";

function loadLate(taskId: string): string[] {
  try {
    const raw = sessionStorage.getItem(LATE_PREFIX + taskId);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function pushLate(taskId: string, text: string) {
  const all = [...loadLate(taskId), text];
  try {
    sessionStorage.setItem(LATE_PREFIX + taskId, JSON.stringify(all.slice(-10)));
  } catch {
    /* 忽略 */
  }
}

/** 工作区输入：草稿按 Task 隔离；模拟延迟发送，迟到结果不覆盖当前输入 */
export function WorkspaceComposer({ taskId, delayMs = 800 }: Props) {
  const key = draftStorageKey("task", taskId);
  const taskRef = useRef(taskId);
  taskRef.current = taskId;
  const aliveRef = useRef(true);

  const [text, setText] = useState("");
  const [version, setVersion] = useState(0);
  const [scope, setScope] = useState<DraftScope>("continue");
  const [continueTaskId, setContinueTaskId] = useState<string | null>(taskId);
  const [sending, setSending] = useState(false);
  const [linked, setLinked] = useState(true);
  const [audits, setAudits] = useState(() => listTaskLinkAudits());
  const [late, setLate] = useState<string[]>([]);
  const [localReply, setLocalReply] = useState<string | null>(null);
  const composingRef = useRef(false);
  const [, bumpComposing] = useState(0);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  useEffect(() => {
    const d = loadDraft(key);
    setText(d.text);
    setVersion(d.version);
    setScope(d.scope === "auto" ? "continue" : d.scope);
    setContinueTaskId(d.continueTaskId ?? taskId);
    setLocalReply(null);
    setLate(loadLate(taskId));
  }, [key, taskId]);

  const writeText = (next: string) => {
    const d = saveDraft(key, { text: next });
    setText(d.text);
    setVersion(d.version);
  };

  const onScope = (s: DraftScope, tid: string | null) => {
    const d = saveDraft(key, {
      text,
      scope: s,
      continueTaskId: s === "continue" ? tid ?? taskId : null,
    });
    setScope(d.scope);
    setContinueTaskId(d.continueTaskId);
  };

  const reason =
    scope === "independent"
      ? "独立话题：普通问答不创建新 Task"
      : scope === "continue"
        ? `继续任务：固定 ${continueTaskId ?? taskId}`
        : "自动关联：按当前工作区任务";

  const mockSend = async () => {
    if (!text.trim() || sending) return;
    const sentVersion = version;
    const sentText = text;
    const boundTask = taskId;
    setSending(true);
    setLocalReply("发送中（模拟延迟）…");
    await new Promise((r) => setTimeout(r, delayMs));
    const note = `已受理（模拟）：${sentText.slice(0, 40)}`;
    const stillHere = aliveRef.current && taskRef.current === boundTask;
    if (stillHere) {
      const d = clearDraftAfterSend(key, sentVersion);
      setText(d.text);
      setVersion(d.version);
      setLocalReply(note);
      setSending(false);
    } else {
      pushLate(boundTask, note);
    }
  };

  const convert = () => {
    appendTaskLinkAudit({ taskId, source: "workspace", action: "link" });
    setLinked(true);
    onScope("continue", taskId);
    setAudits(listTaskLinkAudits());
  };

  const correct = () => {
    appendTaskLinkAudit({ taskId, source: "workspace", action: "revoke" });
    setLinked(false);
    onScope("independent", null);
    setAudits(listTaskLinkAudits());
  };

  return (
    <div className="workspace-composer" data-testid="workspace-composer">
      <ScopeControls
        scope={scope}
        continueTaskId={continueTaskId}
        defaultTaskId={taskId}
        reason={reason}
        onScope={onScope}
        onConvertTask={convert}
        onCorrectLink={correct}
        canConvert={!linked || scope === "independent"}
        canCorrect={linked && scope !== "independent"}
      />
      <div className="composer">
        <textarea
          aria-label="工作区输入"
          data-testid="workspace-input"
          value={text}
          rows={3}
          onChange={(e) => writeText(e.target.value)}
          data-composing={composingRef.current ? "1" : "0"}
          onCompositionStart={() => {
            composingRef.current = true;
            bumpComposing((n) => n + 1);
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
            bumpComposing((n) => n + 1);
          }}
          onKeyDown={(e) => {
            if (e.key !== "Enter" || e.shiftKey) return;
            if (composingRef.current || e.nativeEvent.isComposing || e.keyCode === 229) return;
            e.preventDefault();
            void mockSend();
          }}
          placeholder="在此任务下写草稿；切换任务后仍保留"
        />
        <button
          type="button"
          className="btn primary"
          data-testid="workspace-send"
          disabled={sending || !text.trim()}
          onClick={() => void mockSend()}
        >
          {sending ? "发送中" : "发送"}
        </button>
      </div>
      {localReply && (
        <p className="workspace-reply" data-testid="workspace-reply">
          {localReply}
        </p>
      )}
      {late.map((n, i) => (
        <p key={i} className="workspace-late" data-testid="workspace-late">
          迟到回执：{n}
        </p>
      ))}
      <ul className="scope-audit" data-testid="scope-audit" aria-label="关联修正记录">
        {audits
          .filter((a) => a.taskId === taskId)
          .slice(-5)
          .map((a) => (
            <li key={a.id}>
              {a.action === "link" ? "建立关联" : "撤回关联"} · {a.taskId}
            </li>
          ))}
      </ul>
    </div>
  );
}
