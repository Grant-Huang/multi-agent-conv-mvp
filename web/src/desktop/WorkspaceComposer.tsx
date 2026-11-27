import { useCallback, useEffect, useRef, useState } from "react";
import {
  appendTaskLinkAudit,
  clearDraftAfterSend,
  clearDraftAfterSendRemote,
  draftStorageKey,
  listTaskLinkAudits,
  loadDraft,
  loadDraftRemote,
  onDraftConflict,
  saveDraft,
  scheduleDraftSync,
  type DraftConflict,
  type DraftScope,
  type DraftVariant,
  type TaskLinkAudit,
} from "./draftStore";
import { ScopeControls } from "./ScopeControls";

type Props = {
  taskId: string;
  delayMs?: number;
};

const LATE_PREFIX = "an.desktop.late.v1:";
const SENT_PREFIX = "an.desktop.sent.v1:";

/** UXFIX-D（INTENT-011）：已发送消息与受理回执按任务持久，离开再返回后仍在 */
export type SentEntry = { text: string; note: string; at: number; late?: boolean };

function sentKey(taskId: string): string {
  return `${SENT_PREFIX}task:${taskId}`;
}

function loadSent(taskId: string): SentEntry[] {
  try {
    const raw = sessionStorage.getItem(sentKey(taskId));
    return raw ? (JSON.parse(raw) as SentEntry[]) : [];
  } catch {
    return [];
  }
}

function appendSent(taskId: string, entry: SentEntry): void {
  const all = [...loadSent(taskId), entry];
  try {
    sessionStorage.setItem(sentKey(taskId), JSON.stringify(all.slice(-10)));
  } catch {
    /* 忽略 */
  }
}

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

/** 工作区输入：草稿按 Task 隔离；模拟延迟发送，迟到结果不覆盖当前输入。
 *  UXFIX-D：作用域（继续任务 / 独立话题）分存草稿；跨会话冲突保留本机并提示；
 *  发送记录离开再返回后仍在。 */
export function WorkspaceComposer({ taskId, delayMs = 800 }: Props) {
  const taskRef = useRef(taskId);
  taskRef.current = taskId;
  const aliveRef = useRef(true);

  const [text, setText] = useState("");
  const [version, setVersion] = useState(0);
  const [scope, setScope] = useState<DraftScope>("continue");
  const [continueTaskId, setContinueTaskId] = useState<string | null>(taskId);
  const [sending, setSending] = useState(false);
  const [linked, setLinked] = useState(true);
  const [audits, setAudits] = useState<TaskLinkAudit[]>([]);
  const [auditErr, setAuditErr] = useState<string | null>(null);
  const [late, setLate] = useState<string[]>([]);
  const [sent, setSent] = useState<SentEntry[]>([]);
  const [localReply, setLocalReply] = useState<string | null>(null);
  const [conflict, setConflict] = useState<DraftConflict | null>(null);
  const composingRef = useRef(false);
  const [, bumpComposing] = useState(0);

  const variant: DraftVariant = scope === "independent" ? "independent" : "base";
  const key = draftStorageKey("task", taskId, variant);
  const keyRef = useRef(key);
  keyRef.current = key;

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  // UXFIX（#367）：loadAudits 加任务守卫与请求序号，切任务后旧响应不覆盖新任务数据
  const auditTaskRef = useRef(taskId);
  auditTaskRef.current = taskId;
  const auditSeqRef = useRef(0);

  const loadAudits = useCallback(
    (tid: string) => {
      const seq = ++auditSeqRef.current;
      listTaskLinkAudits(tid)
        .then((result) => {
          if (auditTaskRef.current !== tid || seq !== auditSeqRef.current) return;
          setAudits(result);
          setAuditErr(null);
        })
        .catch((e) => {
          if (auditTaskRef.current !== tid || seq !== auditSeqRef.current) return;
          setAuditErr(String(e));
        });
    },
    [],
  );

  useEffect(() => {
    setLate(loadLate(taskId));
    setSent(loadSent(taskId));
    setAuditErr(null);
    loadAudits(taskId);
  }, [taskId, loadAudits]);

  // 草稿加载（按作用域键）：本地先行，远端刷新；冲突保留本机并提示（UXFIX-D）
  useEffect(() => {
    const d = loadDraft(key);
    setText(d.text);
    setVersion(d.version);
    setConflict(null);
    if (variant === "base") {
      // base 键只承载自动关联 / 继续任务；遗留数据里存过 independent 的钳制为 continue，
      // 独立话题草稿自 UXFIX-D 起分存在 independent 键
      const s = d.scope === "continue" ? "continue" : "auto";
      setScope(s === "auto" ? "continue" : s);
      setContinueTaskId(d.continueTaskId ?? taskId);
    }
    // T-84：以服务端草稿为准刷新（跨设备/清存储后不丢）；冲突时保留本地（UXFIX-D）
    let cancelled = false;
    void loadDraftRemote(key).then((res) => {
      if (cancelled || !aliveRef.current || keyRef.current !== key) return;
      setText(res.draft.text);
      setVersion(res.draft.version);
      if (variant === "base") {
        const s = res.draft.scope === "continue" ? "continue" : "auto";
        setScope(s === "auto" ? "continue" : s);
        setContinueTaskId(res.draft.continueTaskId ?? taskId);
      }
    });
    const off = onDraftConflict(key, (c) => {
      if (!cancelled && aliveRef.current && keyRef.current === key) setConflict(c);
    });
    return () => {
      cancelled = true;
      off();
    };
  }, [key, variant, taskId]);

  const writeText = (next: string) => {
    const d = saveDraft(key, { text: next });
    setText(d.text);
    setVersion(d.version);
    scheduleDraftSync(key);
  };

  const onScope = (s: DraftScope, tid: string | null) => {
    if (s === scope) return;
    // 切换作用域前先把当前文字存回原作用域键（先存后取，INTENT-013）
    saveDraft(key, {
      text,
      scope,
      continueTaskId: scope === "continue" ? continueTaskId ?? taskId : null,
    });
    scheduleDraftSync(key);
    setScope(s);
    setContinueTaskId(s === "continue" ? tid ?? taskId : null);
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
      const d = clearDraftAfterSend(keyRef.current, sentVersion);
      setText(d.text);
      setVersion(d.version);
      clearDraftAfterSendRemote(keyRef.current, sentVersion);
      setLocalReply(note);
      setSending(false);
    } else {
      pushLate(boundTask, note);
    }
    appendSent(boundTask, { text: sentText, note, at: Date.now(), late: !stillHere });
    setSent(loadSent(boundTask));
  };

  const recordAudit = (action: TaskLinkAudit["action"]) => {
    appendTaskLinkAudit(taskId, { source: "workspace", action })
      .then(() => loadAudits(taskId))
      .catch(() => {});
  };

  const convert = () => {
    recordAudit("link");
    setLinked(true);
    onScope("continue", taskId);
  };

  const correct = () => {
    recordAudit("revoke");
    setLinked(false);
    onScope("independent", null);
  };

  const adoptConflictText = () => {
    if (!conflict) return;
    const d = saveDraft(key, { text: conflict.serverText });
    setText(d.text);
    setVersion(d.version);
    scheduleDraftSync(key);
    setConflict(null);
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
      {conflict && (
        <p className="draft-conflict" data-testid="draft-conflict" role="alert">
          {conflict.kind === "kept_local"
            ? "检测到其他设备的草稿版本，已保留本机内容。"
            : "已按版本改为其他设备的草稿，本机先前内容已被替换。"}{" "}
          <button type="button" className="btn small" data-testid="draft-conflict-adopt" onClick={adoptConflictText}>
            {conflict.kind === "kept_local" ? "改用其他设备的草稿" : "恢复本机先前内容"}
          </button>
        </p>
      )}
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
      {sent.length > 0 && (
        <ul className="workspace-sent-log" data-testid="workspace-sent-log" aria-label="发送记录">
          {sent.map((s, i) => (
            <li key={i} className="workspace-sent-item" data-testid="workspace-sent-item">
              发送：{s.text}
              {s.late ? " · 迟到回执：" : " · "}
              {s.note}
            </li>
          ))}
        </ul>
      )}
      {late.map((n, i) => (
        <p key={i} className="workspace-late" data-testid="workspace-late">
          迟到回执：{n}
        </p>
      ))}
      {auditErr && (
        <p className="bad-text" data-testid="scope-audit-error" role="alert">
          审计加载失败：{auditErr}
        </p>
      )}
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
