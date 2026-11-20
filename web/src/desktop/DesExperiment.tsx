// DES 仿真实验（决定 62 / WA M0-2）：配置 → 试跑 → 正式运行 → 结果；对接 /api/world/mock。
import { useEffect, useState } from "react";
import { api, type WorldMockRun } from "../api";

type Tab = "config" | "trial" | "official" | "results";

type Props = {
  onOpenOpsChannel: () => void;
  onBack: () => void;
};

const TABS: { id: Tab; label: string }[] = [
  { id: "config", label: "配置" },
  { id: "trial", label: "试跑" },
  { id: "official", label: "正式运行" },
  { id: "results", label: "结果" },
];

export function DesExperiment({ onOpenOpsChannel, onBack }: Props) {
  const [tab, setTab] = useState<Tab>("config");
  const [factory, setFactory] = useState("demo-factory");
  const [scenario, setScenario] = useState("tool-wear");
  const [seed, setSeed] = useState("42");
  const [duration, setDuration] = useState("480");
  const [configVersion, setConfigVersion] = useState<string | null>(null);
  const [targetChannel, setTargetChannel] = useState("ops-channel");
  const [startKey, setStartKey] = useState("default");
  const [trialRunId, setTrialRunId] = useState<string | null>(null);
  const [trialDelivery, setTrialDelivery] = useState(0);
  const [officialRunId, setOfficialRunId] = useState<string | null>(null);
  const [officialCaseId, setOfficialCaseId] = useState<string | null>(null);
  const [run, setRun] = useState<WorldMockRun | null>(null);
  const [alert, setAlert] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const activeRunId = officialRunId ?? trialRunId;

  useEffect(() => {
    if (!activeRunId) return;
    let cancelled = false;
    const tick = () => {
      api.worldMock
        .getRun(activeRunId)
        .then((r) => {
          if (!cancelled) setRun(r);
        })
        .catch(() => {});
    };
    tick();
    const id = window.setInterval(tick, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [activeRunId]);

  const ensureConfig = async (): Promise<string> => {
    if (configVersion) return configVersion;
    const seedNum = Number(seed);
    const dur = Number(duration);
    const r = await api.worldMock.saveConfig({
      factory,
      scenario,
      seed: Number.isFinite(seedNum) ? seedNum : 0,
      duration_minutes: Number.isFinite(dur) && dur > 0 ? dur : 480,
    });
    setConfigVersion(r.config_version);
    return r.config_version;
  };

  const saveConfig = async () => {
    setBusy(true);
    setAlert(null);
    try {
      const seedNum = Number(seed);
      const dur = Number(duration);
      if (!Number.isFinite(dur) || dur <= 0) {
        setAlert("非法参数：场景时长无效，须为正整数（约束）");
        return;
      }
      const r = await api.worldMock.saveConfig({
        factory,
        scenario,
        seed: Number.isFinite(seedNum) ? seedNum : 0,
        duration_minutes: dur,
      });
      setConfigVersion(r.config_version);
    } catch (e) {
      const msg = String(e);
      setAlert(msg.includes("非法") || msg.includes("无效") || msg.includes("约束") ? msg : `非法参数：${msg}`);
    } finally {
      setBusy(false);
    }
  };

  const startTrial = async () => {
    setBusy(true);
    setAlert(null);
    try {
      const version = await ensureConfig();
      const r = await api.worldMock.startTrial({ config_version: version });
      setTrialRunId(r.trial_run_id);
      setTrialDelivery(r.delivery_count);
      setTab("trial");
    } catch (e) {
      setAlert(String(e));
    } finally {
      setBusy(false);
    }
  };

  const startOfficial = async () => {
    setBusy(true);
    setAlert(null);
    try {
      const version = await ensureConfig();
      const r = await api.worldMock.startOfficial({
        config_version: version,
        target_channel: targetChannel,
        start_key: startKey,
      });
      setOfficialRunId(r.official_run_id);
      setOfficialCaseId(r.case_id);
      setTab("official");
    } catch (e) {
      setAlert(String(e));
    } finally {
      setBusy(false);
    }
  };

  const control = async (action: "pause" | "resume" | "step") => {
    const id = officialRunId ?? trialRunId;
    if (!id) {
      setAlert("非法参数：尚无运行可控制");
      return;
    }
    setBusy(true);
    setAlert(null);
    try {
      const r = await api.worldMock.controlRun(id, {
        action,
        minutes: action === "step" ? 1 : undefined,
      });
      setRun(r);
    } catch (e) {
      setAlert(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="des-experiment" data-testid="des-experiment">
      <header className="des-experiment-header">
        <div className="des-experiment-title-row">
          <button type="button" className="btn small" onClick={onBack}>
            ← 返回桌面
          </button>
          <h1>DES 仿真实验</h1>
          <a
            className="btn small"
            href="#ops-channel"
            onClick={(e) => {
              e.preventDefault();
              onOpenOpsChannel();
            }}
          >
            运营频道
          </a>
        </div>
        <p className="des-experiment-lead">配置场景、试跑校验，再启动正式运行；试跑不向运营频道投递。</p>
      </header>

      <div className="des-tabs" role="tablist" aria-label="DES 实验视图">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={tab === t.id ? "active" : ""}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {alert && (
        <div className="error-bar" role="alert">
          {alert}
        </div>
      )}

      {tab === "config" && (
        <section className="des-panel" aria-label="配置">
          <label className="des-field">
            工厂
            <input value={factory} onChange={(e) => setFactory(e.target.value)} />
          </label>
          <label className="des-field">
            场景
            <input value={scenario} onChange={(e) => setScenario(e.target.value)} />
          </label>
          <label className="des-field">
            种子
            <input value={seed} onChange={(e) => setSeed(e.target.value)} inputMode="numeric" />
          </label>
          <label className="des-field">
            场景时长
            <input
              aria-label="场景时长"
              value={duration}
              onChange={(e) => setDuration(e.target.value)}
              inputMode="numeric"
            />
          </label>
          <div className="des-actions">
            <button type="button" className="btn primary" disabled={busy} onClick={saveConfig}>
              保存配置
            </button>
            <button type="button" className="btn" disabled={busy} onClick={startTrial}>
              试跑
            </button>
          </div>
          {configVersion && (
            <p className="hint">
              配置版本：<span data-testid="config-version-saved">{configVersion}</span>
            </p>
          )}
          {trialRunId && (
            <p className="hint">
              试跑 ID：<span data-testid="trial-run-id">{trialRunId}</span>
              {" · "}
              运营投递：<span data-testid="ops-delivery-count">{trialDelivery}</span>
            </p>
          )}
        </section>
      )}

      {tab === "trial" && (
        <section className="des-panel" aria-label="试跑">
          {!trialRunId ? (
            <p className="hint">先在「配置」保存并点「试跑」。</p>
          ) : (
            <>
              <p>
                试跑 ID：<span data-testid="trial-run-id">{trialRunId}</span>
              </p>
              <p>
                运营频道投递计数：<span data-testid="ops-delivery-count">{trialDelivery}</span>
              </p>
              {run?.mode === "trial" && (
                <p className="hint">
                  状态 {run.run_status} · 仿真分钟 {run.simulation_minute}
                </p>
              )}
              <p className="hint">试跑预览不向运营频道发卡。</p>
            </>
          )}
        </section>
      )}

      {tab === "official" && (
        <section className="des-panel" aria-label="正式运行">
          <p>
            配置版本：
            <span data-testid="config-version">{configVersion ?? "（启动时自动保存默认）"}</span>
          </p>
          <label className="des-field">
            目标频道
            <input
              aria-label="目标频道"
              data-testid="target-channel"
              value={targetChannel}
              onChange={(e) => setTargetChannel(e.target.value)}
            />
          </label>
          <label className="des-field">
            启动键
            <input value={startKey} onChange={(e) => setStartKey(e.target.value)} />
          </label>
          <div className="des-actions">
            <button type="button" className="btn primary" disabled={busy} onClick={startOfficial}>
              启动正式运行
            </button>
            <button type="button" className="btn" disabled={busy || !officialRunId} onClick={() => control("pause")}>
              暂停
            </button>
            <button type="button" className="btn" disabled={busy || !officialRunId} onClick={() => control("step")}>
              单步推进
            </button>
          </div>
          {officialRunId && (
            <>
              <p>
                正式运行 ID：<span data-testid="official-run-id">{officialRunId}</span>
              </p>
              <p>
                上次试跑 ID：<span data-testid="last-trial-run-id">{trialRunId ?? ""}</span>
              </p>
              {officialCaseId && <p className="hint">事项 {officialCaseId}</p>}
              <p
                data-testid="run-status"
                data-status={run?.run_status ?? "running"}
                className="hint"
              >
                {run?.wait_label || run?.run_status || "running"}
              </p>
            </>
          )}
        </section>
      )}

      {tab === "results" && (
        <section className="des-panel" aria-label="结果">
          {!activeRunId ? (
            <p className="hint">尚无运行结果。完成试跑或正式运行后可回看。</p>
          ) : (
            <>
              <p>运行 {activeRunId}</p>
              <p className="hint">
                模式 {run?.mode ?? "—"} · 状态 {run?.run_status ?? "—"} · 投递 {run?.delivery_count ?? "—"}
              </p>
              <p className="hint">评测标准答案仅审核员可见（见 ground-truth 接口）。</p>
            </>
          )}
        </section>
      )}
    </div>
  );
}
