import { useState } from "react";
import {
  readWorkspaceLayout,
  writeWorkspaceLayout,
  type WorkspaceLayout,
  SMT03_TASK_ID,
} from "./mockData";
import { WorkspaceComposer } from "./WorkspaceComposer";

type Props = {
  taskId: string;
  onBack: () => void;
};

export function WorkspaceView({ taskId, onBack }: Props) {
  const [layout, setLayout] = useState<WorkspaceLayout>(() => readWorkspaceLayout());
  const [execOpen, setExecOpen] = useState(true);

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
          <button
            type="button"
            className="btn small"
            aria-expanded={execOpen}
            onClick={() => setExecOpen((v) => !v)}
          >
            {execOpen ? "收起执行" : "展开执行"}
          </button>
          {execOpen && (
            <div className="workspace-exec" data-testid="workspace-exec">
              <p>O-Agent 执行摘要（模拟）：已采集质量记录，等待核验。</p>
            </div>
          )}
          <p className="workspace-hint">草稿与作用域按 Task 隔离（A2）。</p>
          <WorkspaceComposer taskId={taskId} />
        </section>
      </div>
    </div>
  );
}
