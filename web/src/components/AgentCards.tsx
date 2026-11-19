// AG.3 助手相关的四类卡片：发现卡、引导卡、确认卡、结果卡。
// 设计约束（docs/PLAN.md 的 AG 一节）：
// - 只按后端返回的结构化数据渲染，不渲染模型生成的 HTML；
// - 先说大白话，技术细节（命令、修改前后的配置）放在"详细设置"里；
// - 每张卡都写清楚"做什么、为什么、怎么撤销"；
// - 使用水平（uiLevel）先预留：目前一律按"专业"展示，"详细设置"默认展开。
import { useState } from "react";
import type { CatalogItem } from "../bindings/CatalogItem";
import type { InstallStep } from "../bindings/InstallStep";
import type { ProbeOutcome } from "../bindings/ProbeOutcome";
import type { Applied, Proposal } from "../api";
import type { DiagnoseReport } from "../bindings/DiagnoseReport";
import type { DiagnosePreview } from "../bindings/DiagnosePreview";
import type { DiagnoseSendResult } from "../bindings/DiagnoseSendResult";

/** 使用水平：预留给以后的用户分类。现在只实现"专业" */
export type UiLevel = "professional" | "basic";

const UI_LEVEL_KEY = "an.uiLevel";

/** 读取使用水平；读不到（隐私窗口等）时按"专业" */
export function loadUiLevel(): UiLevel {
  try {
    return localStorage.getItem(UI_LEVEL_KEY) === "basic" ? "basic" : "professional";
  } catch {
    return "professional";
  }
}

/** 握手检测结果 → 给非专业用户看的一句话 */
export function probeText(p: ProbeOutcome): string {
  const next = p.next_step ? `下一步：${p.next_step}` : "";
  switch (p.kind) {
    case "ok":
      return "已连通，可以把任务交给它";
    case "not_installed":
      return `还没有安装（本机找不到它的启动命令）。${next}`;
    case "needs_login":
      return `需要先登录。${next}`;
    case "missing_dependency":
      return `缺少运行需要的程序。${next}`;
    case "timeout":
      return `没有在规定时间内响应。${next}`;
    default:
      return `连接失败。${next}`;
  }
}

/** 角色的用途叫法（与后端 role_label 一致） */
export function roleLabel(role: string): string {
  return role === "coding" ? "写代码" : role === "reviewer" ? "审查" : "通用任务";
}

/** 复制按钮：复制失败时提示手动选择，不抛错 */
export function CopyButton(props: { text: string }) {
  const [state, setState] = useState<"" | "ok" | "fail">("");
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.text);
      setState("ok");
    } catch {
      setState("fail");
    }
  };
  return (
    <button type="button" className="btn small" onClick={copy}>
      {state === "ok" ? "已复制" : state === "fail" ? "复制失败，请手动选择" : "复制"}
    </button>
  );
}

/** 发现卡：首次打开时显示在对话区顶部，告诉用户本机有哪些助手 */
export function DiscoveryCard(props: {
  catalog: CatalogItem[];
  /** 首次打开的发现卡才有"知道了"；对话里的发现卡没有 */
  onDismiss?: () => void;
  onManage?: () => void;
}) {
  const connected = props.catalog.filter((c) => c.status === "registered" && !c.disabled);
  const installable = props.catalog.filter((c) => c.status === "installed" && !c.disabled);
  const missing = props.catalog.filter((c) => c.status === "not_installed");
  const line = (c: CatalogItem) => (
    <li key={c.id}>
      <strong>{c.name}</strong>：{c.purpose}
    </li>
  );
  return (
    <section className="agent-card discovery" role="region" aria-label="发现的助手">
      <h3>本机可以用的助手</h3>
      <p className="hint">
        助手负责在后台替你做事（例如写代码、整理文件）。在右侧「助手」页签里可以接入、停用或撤销修改。
      </p>
      {connected.length > 0 && (
        <>
          <h4>已经接入</h4>
          <ul>{connected.map(line)}</ul>
        </>
      )}
      {installable.length > 0 && (
        <>
          <h4>已安装，可以接入</h4>
          <ul>{installable.map(line)}</ul>
        </>
      )}
      {missing.length > 0 && (
        <details>
          <summary>还没安装的（{missing.length} 个）</summary>
          <ul>{missing.map(line)}</ul>
        </details>
      )}
      <div className="card-actions">
        {props.onManage && (
          <button type="button" className="btn small primary" onClick={props.onManage}>
            去「助手」页签管理
          </button>
        )}
        {props.onDismiss && (
          <button type="button" className="btn small" onClick={props.onDismiss}>
            知道了
          </button>
        )}
      </div>
    </section>
  );
}

/** 引导卡：分步安装说明 + 复制命令 + 重新检测 */
export function GuideCard(props: {
  item: CatalogItem;
  onProbe: () => Promise<ProbeOutcome>;
  onClose: () => void;
}) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const recheck = async () => {
    setChecking(true);
    try {
      setResult(probeText(await props.onProbe()));
    } catch (e) {
      setResult(`检测失败：${e}`);
    } finally {
      setChecking(false);
    }
  };
  const steps = props.item.install.filter((s: InstallStep) => s.platform === "all" || s.platform === platform());
  return (
    <section className="agent-card" role="region" aria-label="安装步骤">
      <h3>安装「{props.item.name}」</h3>
      <p className="hint">按顺序在终端里执行下面的步骤，完成后点"我装好了，重新检测"。</p>
      <ol>
        {steps.map((s, i) => (
          <li key={i}>
            {s.text}
            {s.command && (
              <div className="cmd">
                <code>{s.command}</code> <CopyButton text={s.command} />
              </div>
            )}
          </li>
        ))}
      </ol>
      {props.item.requirements.length > 0 && <p className="hint">需要：{props.item.requirements.join("、")}</p>}
      {props.item.docs_url && (
        <p className="hint">
          详细说明：
          <a href={props.item.docs_url} target="_blank" rel="noreferrer">
            {props.item.docs_url}
          </a>
        </p>
      )}
      {result && <p className="result-line">{result}</p>}
      <div className="card-actions">
        <button type="button" className="btn small primary" disabled={checking} onClick={recheck}>
          {checking ? "检测中…" : "我装好了，重新检测"}
        </button>
        <button type="button" className="btn small" onClick={props.onClose}>
          关闭
        </button>
      </div>
    </section>
  );
}

/** 当前浏览器所在平台（只用来挑选安装步骤） */
function platform(): string {
  const p = navigator.platform.toLowerCase();
  return p.includes("mac") ? "macos" : p.includes("win") ? "windows" : "linux";
}

/** 确认卡：说明"将会发生什么"；详细设置里给专业用户看命令和修改前后的配置 */
export function ConfirmCard(props: {
  proposal: Proposal;
  /** 将会执行的命令（接入时才有） */
  command?: string;
  level: UiLevel;
  busy: boolean;
  /** 对话里的确认卡会带状态：applied 已确认、expired 已过期；这两种状态不再显示按钮 */
  state?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const p = props.proposal;
  const done = props.state === "applied" || props.state === "expired";
  return (
    <section className="agent-card confirm" role="region" aria-label="确认修改">
      <h3>确认修改</h3>
      <p className="summary">{p.summary}</p>
      {p.warnings.length > 0 && (
        <ul className="warnings">
          {p.warnings.map((w, i) => (
            <li key={i}>⚠ {w}</li>
          ))}
        </ul>
      )}
      <p className="hint">确认后立即生效；之后可以在「助手」页签点"撤销上一次修改"恢复。这个确认 {Math.round(p.expires_in_secs / 60)} 分钟内有效。</p>
      <details open={props.level === "professional"}>
        <summary>详细设置</summary>
        {props.command && (
          <p>
            将执行的命令：<code>{props.command}</code>
          </p>
        )}
        <div className="diff">
          <div>
            <h4>修改前</h4>
            <pre>{p.before}</pre>
          </div>
          <div>
            <h4>修改后</h4>
            <pre>{p.after}</pre>
          </div>
        </div>
      </details>
      {done ? (
        <p className="result-line">{props.state === "applied" ? "已确认，修改已经生效" : "已过期，没有生效；需要的话请重新发起"}</p>
      ) : (
        <div className="card-actions">
          <button type="button" className="btn small primary" disabled={props.busy} onClick={props.onConfirm}>
            确认
          </button>
          <button type="button" className="btn small" disabled={props.busy} onClick={props.onCancel}>
            取消
          </button>
        </div>
      )}
    </section>
  );
}

/** 结果卡：大白话说明结果 + 新接入助手的检测结果 + 撤销 */
export function ResultCard(props: {
  applied?: Applied;
  error?: string;
  canUndo: boolean;
  onUndo: () => void;
  onClose: () => void;
}) {
  return (
    <section className="agent-card result" role="region" aria-label="修改结果">
      <h3>{props.error ? "没有生效" : "已完成"}</h3>
      {props.error ? (
        <p className="error-text">{props.error}</p>
      ) : (
        <>
          <p className="summary">{props.applied?.summary}</p>
          {props.applied?.probes.map((p) => (
            <p key={p.id} className="result-line">
              检测「{p.id}」：{probeText(p)}
            </p>
          ))}
        </>
      )}
      <div className="card-actions">
        {!props.error && props.canUndo && (
          <button type="button" className="btn small" onClick={props.onUndo}>
            撤销
          </button>
        )}
        <button type="button" className="btn small" onClick={props.onClose}>
          关闭
        </button>
      </div>
    </section>
  );
}

const CHECK_MARK: Record<string, string> = { ok: "✓", problem: "✗", skipped: "–", error: "!" };

/** 可以当帮手的助手（已接入、已连通、不是出问题的那个） */
export type HelperOption = { id: string; name: string };

/** 排查结果卡（AG.5）：
 * - 第 1、2 层：结论和修复步骤、本机体检的每一项（全部在本机完成）；
 * - 第 3 层：诊断包，用户自己决定复制给谁；
 * - 第 4 层：请其他助手帮忙查。先给用户看将发送的完整内容和目的地，用户同意这一次才发。 */
export function DiagnoseCard(props: {
  report: DiagnoseReport;
  helpers: HelperOption[];
  onPreview: (helper: string) => Promise<DiagnosePreview>;
  onSend: (p: DiagnosePreview) => Promise<DiagnoseSendResult>;
  onRecheck: () => void;
  onClose: () => void;
}) {
  const r = props.report;
  const pkg = r.package;
  const [helper, setHelper] = useState(props.helpers[0]?.id ?? "");
  const [preview, setPreview] = useState<DiagnosePreview | null>(null);
  const [reply, setReply] = useState<DiagnoseSendResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const makePreview = async () => {
    setBusy(true);
    setError(null);
    setReply(null);
    try {
      setPreview(await props.onPreview(helper));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const sendOnce = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      setReply(await props.onSend(preview));
      setPreview(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPreview(null);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="agent-card" role="region" aria-label="排查结果">
      <h3>排查「{r.name}」</h3>
      <p className="result-line">检测结果：{probeText(r.probe)}</p>
      {r.findings.length > 0 && <h4>可能的原因和解决办法</h4>}
      {r.findings.map((f) => (
        <div key={f.rule} className="finding">
          <p className="summary">{f.title}</p>
          <p className="hint">{f.explain}</p>
          <ol>
            {f.steps.map((s, i) => (
              <li key={i}>
                {s.text}
                {s.command && (
                  <div className="cmd">
                    <code>{s.command}</code> <CopyButton text={s.command} />
                  </div>
                )}
              </li>
            ))}
          </ol>
        </div>
      ))}
      {r.unmatched && (
        <p className="hint">没有找到已知的原因。可以把上面的检测结果和下面的体检信息发给熟悉的人或工具帮忙看看。</p>
      )}
      {r.probe.kind === "ok" && <p className="hint">没有发现问题。</p>}
      <details open>
        <summary>本机体检（只做了只读检查，没有往外发送任何内容）</summary>
        <ul className="checks">
          {r.checks.map((c) => (
            <li key={c.key} className={`check ${c.status}`}>
              {CHECK_MARK[c.status] ?? "·"} {c.name}：{c.detail}
            </li>
          ))}
        </ul>
      </details>
      {r.probe.kind !== "ok" && (
        <section role="region" aria-label="诊断包" className="diag-package">
          <h4>诊断包</h4>
          <p className="hint">{pkg.note}</p>
          {pkg.text && (
            <>
              <pre>{pkg.text}</pre>
              <CopyButton text={pkg.text} />
            </>
          )}
        </section>
      )}
      {r.probe.kind !== "ok" && (
        <div className="diag-helper">
          <h4>请其他助手帮忙查</h4>
          <p className="hint">会先给你看将发送的完整内容和发给谁；你同意这一次才发送，不会自动再发。</p>
          {props.helpers.length === 0 ? (
            <p className="hint">现在没有已连通的其他助手可以当帮手。</p>
          ) : (
            <div className="card-actions">
              <label>
                帮手{" "}
                <select aria-label="帮手" value={helper} onChange={(e) => setHelper(e.target.value)} disabled={busy}>
                  {props.helpers.map((h) => (
                    <option key={h.id} value={h.id}>
                      {h.name}
                    </option>
                  ))}
                </select>
              </label>
              <button type="button" className="btn small" disabled={busy || !helper} onClick={makePreview}>
                生成预览
              </button>
            </div>
          )}
          {preview && (
            <section role="region" aria-label="外发预览" className="diag-preview">
              <p className="summary">{preview.destination}</p>
              <p className="hint">下面就是将发送的全部内容，一字不差。这次同意 {Math.round(preview.expires_in_secs / 60)} 分钟内有效。</p>
              <pre>{preview.content}</pre>
              <div className="card-actions">
                <button type="button" className="btn small primary" disabled={busy} onClick={sendOnce}>
                  {busy ? "发送中…" : "同意发送这一次"}
                </button>
                <button type="button" className="btn small" disabled={busy} onClick={() => setPreview(null)}>
                  取消
                </button>
              </div>
            </section>
          )}
          {reply && (
            <section role="region" aria-label="帮手的回复" className="diag-reply">
              <h4>「{reply.helper}」的回复</h4>
              <pre>{reply.reply}</pre>
              {!reply.restored && <p className="hint">回复里的占位符没能还原，按原样显示。</p>}
            </section>
          )}
          {error && <p className="error-text">{error}</p>}
        </div>
      )}
      <div className="card-actions">
        <button type="button" className="btn small primary" onClick={props.onRecheck}>
          重新排查
        </button>
        <button type="button" className="btn small" onClick={props.onClose}>
          关闭
        </button>
      </div>
    </section>
  );
}
