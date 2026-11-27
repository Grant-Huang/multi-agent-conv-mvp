import { useEffect, useState } from "react";
import {
  readWorkspaceLayout,
  writeWorkspaceLayout,
  type WorkspaceLayout,
  SMT03_TASK_ID,
  FIXTURE_TASK_IDS,
} from "./mockData";
import { loadExecOpen, saveExecOpen } from "./readAnchorStore";
import { taskStatusSubtitle } from "./taskStatusText";
import { api } from "../api";
import type { TaskRecord } from "../bindings/TaskRecord";
import { WorkspaceComposer } from "./WorkspaceComposer";
import { ApprovalMockPanel } from "./ApprovalMockPanel";
import { ResumeMockPanel } from "./ResumeMockPanel";
import { VoiceMockPanel } from "./VoiceMockPanel";
import { HandoffMockPanel } from "./HandoffMockPanel";
import { AcceptMockPanel } from "./AcceptMockPanel";

type Props = {
  taskId: string;
  onBack: () => void;
  onOpenEvidence?: () => void;
  onOpenSettings?: () => void;
  onOpenTask?: (taskId: string) => void;
  /** UXFIX-B（INTENT-003）：服务端告知的测试模式；仅在 true 时渲染模拟测试控件 */
  testFixtures?: boolean;
};

/** 真实任务信息加载态：null=加载中，"error"=加载失败（CH-P3：出错就报错，不回退模拟） */
type RealTaskState = { task: TaskRecord } | "loading" | "error";

export function WorkspaceView({ taskId, onBack, onOpenEvidence, onOpenSettings, onOpenTask, testFixtures = false }: Props) {
  const [layout, setLayout] = useState<WorkspaceLayout>(() => readWorkspaceLayout());
  const [execOpen, setExecOpen] = useState(() => loadExecOpen(taskId));
  const [showAppr, setShowAppr] = useState(false);
  const [showResume, setShowResume] = useState(false);
  const [showVoice, setShowVoice] = useState(false);
  const [showHandoff, setShowHandoff] = useState(false);
  const [showAccept, setShowAccept] = useState(false);
  // UXFIX-E（INTENT-014）：任务信息与执行摘要按 Task ID 读取真实任务；夹具任务保留模拟内容
  const isFixture = FIXTURE_TASK_IDS.has(taskId);
  const [realTask, setRealTask] = useState<RealTaskState>("loading");

  useEffect(() => {
    setExecOpen(loadExecOpen(taskId));
  }, [taskId]);

  useEffect(() => {
    if (isFixture) return;
    let cancelled = false;
    setRealTask("loading");
    api
      .task(taskId)
      .then((r) => {
        if (!cancelled) setRealTask({ task: r.task });
      })
      .catch(() => {
        if (!cancelled) setRealTask("error");
      });
    return () => {
      cancelled = true;
    };
  }, [taskId, isFixture]);

  // UXFIX（#520 M11）：运行中任务定时重读，状态不再停留「Agent 执行中」
  useEffect(() => {
    if (isFixture) return;
    let cancelled = false;
    const t = setInterval(() => {
      api
        .task(taskId)
        .then((r) => {
          if (!cancelled) {
            setRealTask((prev) => {
              if (prev === "loading" || prev === "error") return prev;
              if (prev.task.status !== r.task.status) return { task: r.task };
              return prev;
            });
          }
        })
        .catch(() => {});
    }, 5_000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [taskId, isFixture]);

  const toggleExec = () => {
    setExecOpen((v) => {
      const next = !v;
      saveExecOpen(taskId, next);
      return next;
    });
  };

  const setAndPersist = (next: WorkspaceLayout) => {
    setLayout(next);
    writeWorkspaceLayout(next);
  };

  return (
    <div
      className={`desktop-workspace layout-${layout}`}
      data-testid="workspace"
      data-layout={layout}
    >
      <header className="workspace-bar">
        <button type="button" className="btn small" onClick={onBack}>
          返回桌面
        </button>
        <h2 className="workspace-title">
          {taskId === SMT03_TASK_ID ? "SMT-03 质量调查" : "任务"}{" "}
          <span data-testid="workspace-task-id">{taskId}</span>
        </h2>
        <div className="workspace-layout-toggle" role="group" aria-label="工作区布局">
          {onOpenEvidence && (
            <button type="button" className="btn small" data-testid="workspace-open-evidence" onClick={onOpenEvidence}>
              证据
            </button>
          )}
          {onOpenSettings && (
            <button type="button" className="btn small" data-testid="workspace-open-settings" onClick={onOpenSettings}>
              调整助理
            </button>
          )}
          <button
            type="button"
            className={layout === "s2" ? "btn small active" : "btn small"}
            aria-pressed={layout === "s2"}
            onClick={() => setAndPersist("s2")}
          >
            分屏 S2
          </button>
          <button
            type="button"
            className={layout === "s3" ? "btn small active" : "btn small"}
            aria-pressed={layout === "s3"}
            onClick={() => setAndPersist("s3")}
          >
            专注 S3
          </button>
        </div>
      </header>
      <div className="workspace-body">
        {layout === "s2" && (
          <aside className="workspace-task-pane" data-testid="workspace-task-pane" aria-label="任务信息">
            {isFixture ? (
              <dl>
                <dt>状态</dt>
                <dd>执行中（模拟）</dd>
                <dt>业务对象</dt>
                <dd>质量批次 SMT-03</dd>
                <dt>来源</dt>
                <dd>桌面首页 Mock</dd>
              </dl>
            ) : realTask === "loading" ? (
              <p className="muted" data-testid="workspace-task-loading">
                任务信息加载中…
              </p>
            ) : realTask === "error" ? (
              <p className="bad-text" data-testid="workspace-task-error" role="alert">
                任务信息加载失败，请返回桌面重试。
              </p>
            ) : (
              <dl>
                <dt>状态</dt>
                <dd data-testid="workspace-task-status">{taskStatusSubtitle(realTask.task.status)}</dd>
                <dt>目标</dt>
                <dd data-testid="workspace-task-goal">{realTask.task.goal}</dd>
                <dt>产物</dt>
                <dd data-testid="workspace-task-artifacts">
                  {realTask.task.result && realTask.task.result.artifacts.length > 0
                    ? realTask.task.result.artifacts.map((a) => a.name).join("、")
                    : "暂无产物"}
                </dd>
              </dl>
            )}
          </aside>
        )}
        <section className="workspace-session" aria-label="会话与执行">
          {/* CH-U7（#321）：输入框固定在会话栏可视位置，内容区单独滚动 */}
          <div className="workspace-session-scroll">
          <button
            type="button"
            className="btn small"
            data-testid="workspace-exec-toggle"
            aria-expanded={execOpen}
            onClick={toggleExec}
          >
            {execOpen ? "收起执行" : "展开执行"}
          </button>
          {execOpen && (
            <div className="workspace-exec" data-testid="workspace-exec">
              {isFixture ? (
                <p>O-Agent 执行摘要（模拟）：已采集质量记录，等待核验。</p>
              ) : realTask === "error" ? (
                <p className="bad-text">执行信息加载失败。</p>
              ) : realTask === "loading" ? (
                <p className="muted">执行信息加载中…</p>
              ) : (
                <p data-testid="workspace-exec-summary">
                  {realTask.task.result?.summary?.trim()
                    ? realTask.task.result.summary
                    : "还没有执行记录。"}
                </p>
              )}
            </div>
          )}
          {/* UXFIX-B：模拟测试控件只在测试模式渲染（服务端告知），正常界面不出现（CH-U5） */}
          {testFixtures && (
            <>
              <div className="workspace-mock-toggles" data-testid="workspace-mock-toggles">
            <button
              type="button"
              className="btn small"
              data-testid="workspace-open-appr"
              aria-expanded={showAppr}
              onClick={() => setShowAppr((v) => !v)}
            >
              {showAppr ? "收起审批模拟" : "动作审批模拟"}
            </button>
            <button
              type="button"
              className="btn small"
              data-testid="workspace-open-resume"
              aria-expanded={showResume}
              onClick={() => setShowResume((v) => !v)}
            >
              {showResume ? "收起 Run 恢复" : "异步 Run 恢复"}
            </button>
            <button
              type="button"
              className="btn small"
              data-testid="workspace-open-voice"
              aria-expanded={showVoice}
              onClick={() => setShowVoice((v) => !v)}
            >
              {showVoice ? "收起语音模拟" : "语音模拟"}
            </button>
            <button
              type="button"
              className="btn small"
              data-testid="workspace-open-handoff"
              aria-expanded={showHandoff}
              onClick={() => setShowHandoff((v) => !v)}
            >
              {showHandoff ? "收起跨端续接" : "跨端续接模拟"}
            </button>
            <button
              type="button"
              className="btn small"
              data-testid="workspace-open-accept"
              aria-expanded={showAccept}
              onClick={() => setShowAccept((v) => !v)}
            >
              {showAccept ? "收起受理模拟" : "受理模拟"}
            </button>
          </div>
          {showAppr && <ApprovalMockPanel taskId={taskId} />}
          {showResume && <ResumeMockPanel onOpenTask={onOpenTask} />}
          {showVoice && <VoiceMockPanel taskId={taskId} />}
          {showHandoff && <HandoffMockPanel taskId={taskId} />}
          {showAccept && <AcceptMockPanel taskId={taskId} />}
            </>
          )}
          <p className="workspace-hint">草稿与作用域按 Task 隔离（A2）。</p>
          </div>
          {/* UXFIX（#520 M10）：key=taskId 切任务强制重挂载，杜绝旧任务状态串入 */}
          <WorkspaceComposer key={taskId} taskId={taskId} />
        </section>
      </div>
    </div>
  );
}
