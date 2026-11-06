// 两步确认按钮：第一次点击变为确认文字，3 秒内再点才执行。
// 用来替代 window.confirm()，确认提示留在界面里，不弹出阻塞式对话框。
import { useEffect, useState } from "react";

export function ConfirmButton(props: {
  label: string;
  confirmLabel: string;
  onConfirm: () => void;
  disabled?: boolean;
  className?: string;
}) {
  const { label, confirmLabel, onConfirm, disabled, className = "btn small" } = props;
  const [armed, setArmed] = useState(false);

  // 3 秒后自动恢复，避免用户过了很久才点时误触发
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 3000);
    return () => clearTimeout(t);
  }, [armed]);

  return (
    <button
      type="button"
      className={`${className} ${armed ? "danger" : ""}`}
      disabled={disabled}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirmLabel : label}
    </button>
  );
}
