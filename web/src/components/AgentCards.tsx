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
function CopyButton(props: { text: string }) {
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
