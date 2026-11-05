// 路由日志：每轮对话的 Turn 决策、Router 决策、Policy 结果、重试次数、延迟、模型原始输出。
// 用来观察小模型的路由表现，也是积累评测集的素材。
import { useEffect, useState } from "react";
import { api } from "../api";
import type { RouteLog } from "../bindings/RouteLog";

export function RouteLogPanel({ refreshTick }: { refreshTick: number }) {
  const [logs, setLogs] = useState<RouteLog[]>([]);

  const load = () => api.routeLogs().then(setLogs).catch(() => {});

  // 打开页签时拉一次；之后只在状态变化 / 助手消息时节流刷新
  useEffect(() => {
    load();
  }, [refreshTick]);

  return (
    <div className="task-list">
      <div className="row">
        <button className="btn small" onClick={load}>刷新</button>
      </div>
      {logs.length === 0 && <div className="panel-empty">暂无路由日志。</div>}
      {logs.map((l) => (
        <div key={l.id} className="task-card log">
          <div className="log-text">{l.user_text}</div>
          <div className="kv">
            <b>{l.turn_decision?.action ?? "解析失败"}</b>
            {l.route_decision && ` → ${l.route_decision.execution_role} / ${l.route_decision.task_class} / 风险 ${l.route_decision.risk_level}`}
          </div>
          <div className={`kv ${l.attempts > 2 ? "warn-text" : ""}`}>
            结构化调用 {l.attempts} 次 · {(l.latency_ms / 1000).toFixed(1)}s
            {l.policy && !l.policy.auto_execute && " · 需审批"}
          </div>
          {l.error && <div className="kv bad-text">{l.error}</div>}
          <details>
            <summary>模型原始输出（{l.raw_outputs.length}）</summary>
            {l.raw_outputs.map((r, i) => (
              <pre key={i}>{r}</pre>
            ))}
          </details>
        </div>
      ))}
    </div>
  );
}
