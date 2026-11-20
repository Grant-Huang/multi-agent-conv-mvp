import type { DraftScope } from "./draftStore";

type Props = {
  scope: DraftScope;
  continueTaskId: string | null;
  defaultTaskId: string | null;
  reason: string;
  onScope: (scope: DraftScope, continueTaskId: string | null) => void;
  onConvertTask: () => void;
  onCorrectLink: () => void;
  canConvert: boolean;
  canCorrect: boolean;
};

export function ScopeControls({
  scope,
  continueTaskId,
  defaultTaskId,
  reason,
  onScope,
  onConvertTask,
  onCorrectLink,
  canConvert,
  canCorrect,
}: Props) {
  return (
    <div className="scope-controls" data-testid="scope-controls">
      <div className="scope-row" role="group" aria-label="上下文作用域">
        <button
          type="button"
          className={scope === "auto" ? "btn small active" : "btn small"}
          aria-pressed={scope === "auto"}
          onClick={() => onScope("auto", null)}
        >
          自动关联
        </button>
        <button
          type="button"
          className={scope === "continue" ? "btn small active" : "btn small"}
          aria-pressed={scope === "continue"}
          onClick={() => onScope("continue", continueTaskId ?? defaultTaskId)}
        >
          继续当前任务
        </button>
        <button
          type="button"
          className={scope === "independent" ? "btn small active" : "btn small"}
          aria-pressed={scope === "independent"}
          onClick={() => onScope("independent", null)}
        >
          独立话题
        </button>
      </div>
      <p className="scope-reason" data-testid="scope-reason">
        {reason}
      </p>
      {scope === "continue" && (continueTaskId || defaultTaskId) && (
        <p className="scope-task" data-testid="scope-task-id">
          固定任务 {continueTaskId ?? defaultTaskId}
        </p>
      )}
      <div className="scope-actions">
        {canConvert && (
          <button type="button" className="btn small" onClick={onConvertTask}>
            转为任务
          </button>
        )}
        {canCorrect && (
          <button type="button" className="btn small" onClick={onCorrectLink}>
            纠正关联
          </button>
        )}
      </div>
    </div>
  );
}
