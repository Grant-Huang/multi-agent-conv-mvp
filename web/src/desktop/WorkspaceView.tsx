import { useEffect, useState } from "react";
import {
  readWorkspaceLayout,
  writeWorkspaceLayout,
  type WorkspaceLayout,
  SMT03_TASK_ID,
} from "./mockData";
import { loadExecOpen, saveExecOpen } from "./readAnchorStore";
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
};

export function WorkspaceView({ taskId, onBack, onOpenEvidence, onOpenSettings, onOpenTask }: Props) {
  const [layout, setLayout] = useState<WorkspaceLayout>(() => readWorkspaceLayout());
  const [execOpen, setExecOpen] = useState(() => loadExecOpen(taskId));
  const [showAppr, setShowAppr] = useState(false);
  const [showResume, setShowResume] = useState(false);
  const [showVoice, setShowVoice] = useState(false);
  const [showHandoff, setShowHandoff] = useState(false);
  const [showAccept, setShowAccept] = useState(false);

  useEffect(() => {
    setExecOpen(loadExecOpen(taskId));
  }, [taskId]);

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
            <dl>
              <dt>状态</dt>
              <dd>执行中（模拟）</dd>
              <dt>业务对象</dt>
              <dd>质量批次 SMT-03</dd>
              <dt>来源</dt>
              <dd>桌面首页 Mock</dd>
            </dl>
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
              <p>O-Agent 执行摘要（模拟）：已采集质量记录，等待核验。</p>
            </div>
          )}
          <div className="workspace-mock-toggles">
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
          <p className="workspace-hint">草稿与作用域按 Task 隔离（A2）。</p>
          </div>
          <WorkspaceComposer taskId={taskId} />
        </section>
      </div>
    </div>
  );
}
