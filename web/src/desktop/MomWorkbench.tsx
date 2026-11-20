import { useState } from "react";
import {
  MOM_WORLDS,
  casesForWorld,
  findCase,
  type MomCase,
  type MomWorld,
} from "./momWorkbenchData";

export type MomEntry = "board" | "task" | "resource";

type Props = {
  onBack: () => void;
  onOpenOpsChannel?: (caseId: string) => void;
  onOpenDesExperiment?: () => void;
  /** 从外部跳入时预选（如 DES 回跳） */
  initialWorldId?: string;
  initialCaseId?: string;
  initialEntry?: MomEntry;
};

const ENTRY_LABEL: Record<MomEntry, string> = {
  board: "看板",
  task: "任务",
  resource: "资源",
};

const WORLD_KEY = "mom-workbench-world";
const ENTRY_KEY = "mom-workbench-entry";

function readStoredWorld(): string {
  try {
    return sessionStorage.getItem(WORLD_KEY) ?? MOM_WORLDS[0].world_id;
  } catch {
    return MOM_WORLDS[0].world_id;
  }
}

function readStoredEntry(): MomEntry {
  try {
    const v = sessionStorage.getItem(ENTRY_KEY);
    if (v === "board" || v === "task" || v === "resource") return v;
  } catch {
    /* ignore */
  }
  return "board";
}

function provenanceLabel(w: MomWorld): string {
  if (w.provenance === "des") return "DES";
  if (w.provenance === "production") return "正式";
  return "模拟";
}

function CaseCard({
  c,
  selected,
  onOpen,
}: {
  c: MomCase;
  selected: boolean;
  onOpen: () => void;
}) {
  return (
    <button
      type="button"
      className={`mom-case-card${selected ? " selected" : ""}`}
      data-case-id={c.case_id}
      aria-label={`事项 ${c.title}`}
      onClick={onOpen}
    >
      <div className="mom-case-title">{c.title}</div>
      <div className="mom-case-meta">
        <span>{c.stage}</span>
        <span>{c.owner}</span>
        <span>{c.impact}</span>
      </div>
      <div className="mom-case-next">下一步：{c.next_step}</div>
      {c.resource_gaps.length > 0 && (
        <div className="mom-case-gaps" aria-label="资源缺口">
          {c.resource_gaps.map((g) => (
            <span key={g} className="mom-gap">
              {g}
            </span>
          ))}
        </div>
      )}
    </button>
  );
}

export function MomWorkbench({
  onBack,
  onOpenOpsChannel,
  onOpenDesExperiment,
  initialWorldId,
  initialCaseId,
  initialEntry,
}: Props) {
  const [worldId, setWorldId] = useState(
    () => initialWorldId ?? readStoredWorld(),
  );
  const [entry, setEntry] = useState<MomEntry>(
    () => initialEntry ?? readStoredEntry(),
  );
  const [caseId, setCaseId] = useState<string | null>(
    () => initialCaseId ?? null,
  );

  const world = MOM_WORLDS.find((w) => w.world_id === worldId) ?? MOM_WORLDS[0];
  const cases = casesForWorld(world.world_id);
  const selected =
    caseId && cases.some((c) => c.case_id === caseId)
      ? findCase(caseId)
      : undefined;

  function selectWorld(id: string) {
    setWorldId(id);
    setCaseId(null);
    try {
      sessionStorage.setItem(WORLD_KEY, id);
    } catch {
      /* ignore */
    }
  }

  function selectEntry(e: MomEntry) {
    setEntry(e);
    try {
      sessionStorage.setItem(ENTRY_KEY, e);
    } catch {
      /* ignore */
    }
  }

  function openCase(id: string) {
    setCaseId(id);
  }

  const boardCases = cases;
  const taskCases: Record<MomCase["task_group"], MomCase[]> = {
    mine: [],
    running: [],
    blocked: [],
    done: [],
  };
  for (const c of cases) taskCases[c.task_group].push(c);
  const resourceCases = cases.filter((c) => c.resource_gaps.length > 0);

  return (
    <div className="mom-workbench" data-testid="mom-workbench">
      <header className="mom-wb-header">
        <button type="button" className="ghost" onClick={onBack}>
          ← 返回桌面
        </button>
        <h1>MOM 工作台</h1>
        <span className="mom-provenance" aria-label="数据来源">
          {provenanceLabel(world)}
        </span>
      </header>

      <div className="mom-wb-toolbar">
        <label>
          选择世界
          <select
            aria-label="选择世界"
            value={world.world_id}
            onChange={(e) => selectWorld(e.target.value)}
          >
            {MOM_WORLDS.map((w) => (
              <option key={w.world_id} value={w.world_id}>
                {w.label}
              </option>
            ))}
          </select>
        </label>
        <nav className="mom-entry-tabs" aria-label="工作台入口">
          {(Object.keys(ENTRY_LABEL) as MomEntry[]).map((e) => (
            <button
              key={e}
              type="button"
              className={entry === e ? "active" : ""}
              aria-label={ENTRY_LABEL[e]}
              aria-pressed={entry === e}
              onClick={() => selectEntry(e)}
            >
              {ENTRY_LABEL[e]}
            </button>
          ))}
        </nav>
        <span className="mom-wb-ids" aria-label="当前世界标识">
          world_id={world.world_id}
        </span>
      </div>

      <div className="mom-wb-body">
        <section className="mom-wb-list" aria-label={`${ENTRY_LABEL[entry]}列表`}>
          {entry === "board" && (
            <div className="mom-board-grid">
              {boardCases.map((c) => (
                <CaseCard
                  key={c.case_id}
                  c={c}
                  selected={c.case_id === caseId}
                  onOpen={() => openCase(c.case_id)}
                />
              ))}
              {boardCases.length === 0 && (
                <p className="muted">该世界暂无看板事项</p>
              )}
            </div>
          )}
          {entry === "task" && (
            <div className="mom-task-groups">
              {(
                [
                  ["mine", "我的待办"],
                  ["running", "进行中"],
                  ["blocked", "阻塞"],
                  ["done", "已完成"],
                ] as const
              ).map(([key, label]) => (
                <div key={key} className="mom-task-group">
                  <h3>{label}</h3>
                  {taskCases[key].map((c) => (
                    <CaseCard
                      key={c.case_id}
                      c={c}
                      selected={c.case_id === caseId}
                      onOpen={() => openCase(c.case_id)}
                    />
                  ))}
                  {taskCases[key].length === 0 && (
                    <p className="muted">无</p>
                  )}
                </div>
              ))}
            </div>
          )}
          {entry === "resource" && (
            <div className="mom-resource-list">
              <p className="muted">按人机料法环测缺口聚合（模拟）</p>
              {resourceCases.map((c) => (
                <CaseCard
                  key={c.case_id}
                  c={c}
                  selected={c.case_id === caseId}
                  onOpen={() => openCase(c.case_id)}
                />
              ))}
              {resourceCases.length === 0 && (
                <p className="muted">该世界暂无资源缺口</p>
              )}
            </div>
          )}
        </section>

        <aside className="mom-wb-detail" aria-label="事项页">
          {!selected && (
            <p className="muted">从{ENTRY_LABEL[entry]}选一项查看详情</p>
          )}
          {selected && (
            <article
              className="mom-case-page"
              data-testid="mom-case-page"
              data-case-id={selected.case_id}
            >
              <header>
                <h2>{selected.title}</h2>
                <p className="mom-case-ids">
                  case_id=<code>{selected.case_id}</code>
                  {" · "}
                  world_id=<code>{selected.world_id}</code>
                </p>
              </header>
              <dl className="mom-case-dl">
                <div>
                  <dt>阶段</dt>
                  <dd>{selected.stage}</dd>
                </div>
                <div>
                  <dt>负责人</dt>
                  <dd>{selected.owner}</dd>
                </div>
                <div>
                  <dt>影响</dt>
                  <dd>{selected.impact}</dd>
                </div>
                <div>
                  <dt>下一步</dt>
                  <dd>{selected.next_step}</dd>
                </div>
              </dl>
              <p>{selected.summary}</p>
              {selected.resource_gaps.length > 0 && (
                <p>
                  资源缺口：{selected.resource_gaps.join("、")}
                </p>
              )}
              <div className="mom-case-actions">
                {onOpenOpsChannel && (
                  <button
                    type="button"
                    className="primary"
                    aria-label="前往运营频道"
                    onClick={() => onOpenOpsChannel(selected.case_id)}
                  >
                    前往运营频道
                  </button>
                )}
                {world.provenance === "des" && onOpenDesExperiment && (
                  <button
                    type="button"
                    className="ghost"
                    aria-label="打开 DES 实验台"
                    onClick={onOpenDesExperiment}
                  >
                    打开 DES 实验台
                  </button>
                )}
              </div>
              <p className="muted small">
                看板 / 任务 / 资源共用同一 case_id；切换入口不改事项标识。
              </p>
            </article>
          )}
        </aside>
      </div>
    </div>
  );
}
