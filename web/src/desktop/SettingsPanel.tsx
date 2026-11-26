import { useEffect, useRef, useState } from "react";
import { AgentsPanel } from "../components/AgentsPanel";
import { DiscoveryCard } from "../components/AgentCards";
import type { CatalogItem } from "../bindings/CatalogItem";
import { api } from "../api";
import { readAgentsIntroSeen, writeAgentsIntroSeen } from "./agentIntro";
import {
  SETTINGS_CATEGORIES,
  bumpProfileVersionForConflict,
  correctMemory,
  fetchMemories,
  fetchMemory,
  fetchProfile,
  saveProfile,
  withdrawMemory,
  type AssistantProfile,
  type MemoryEntry,
  type SettingsCategoryId,
} from "./profileStore";

export type SettingsLayer = "categories" | "assistant" | "memory";

type Props = {
  layer: SettingsLayer;
  categoryId: SettingsCategoryId | null;
  memoryId: string | null;
  search: string;
  onNavigate: (next: {
    layer: SettingsLayer;
    categoryId: SettingsCategoryId | null;
    memoryId: string | null;
    search?: string;
  }) => void;
  onClose: () => void;
  onProfileSaved: (profile: AssistantProfile) => void;
  /** UXFIX-B（INTENT-003）：服务端告知的测试模式；仅在 true 时渲染“注入冲突（测试）” */
  testFixtures?: boolean;
};

export function SettingsPanel({
  layer,
  categoryId,
  memoryId,
  search,
  onNavigate,
  onClose,
  onProfileSaved,
  testFixtures = false,
}: Props) {
  const [profile, setProfile] = useState<AssistantProfile | null>(null);
  const [draft, setDraft] = useState<AssistantProfile | null>(null);
  const [baseVersion, setBaseVersion] = useState(1);
  const [storedVersion, setStoredVersion] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const [loading, setLoading] = useState(false);
  // T-88：连接与能力分类 = 助手发现卡（首次）+ 助手管理列表
  const [connectCatalog, setConnectCatalog] = useState<CatalogItem[] | null>(null);
  const [connectCardDismissed, setConnectCardDismissed] = useState(() => readAgentsIntroSeen());

  useEffect(() => {
    if (layer !== "categories" || categoryId !== "connect" || connectCardDismissed || connectCatalog) return;
    api
      .agentCatalog()
      .then(setConnectCatalog)
      .catch(() => setConnectCardDismissed(true));
  }, [layer, categoryId, connectCardDismissed, connectCatalog]);
  const [injecting, setInjecting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [memories, setMemories] = useState<MemoryEntry[]>([]);
  const [memoryDetail, setMemoryDetail] = useState<MemoryEntry | null>(null);
  const [memoryError, setMemoryError] = useState<string | null>(null);
  const [memoryLoading, setMemoryLoading] = useState(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;

  useEffect(() => {
    if (layer !== "assistant") return;
    // 已有编辑态时不重拉（从记忆返回不得覆盖未保存草稿）
    if (draftRef.current !== null) return;
    let cancelled = false;
    setLoading(true);
    void fetchProfile()
      .then((p) => {
        if (cancelled) return;
        setProfile(p);
        setDraft(p);
        setBaseVersion(p.version);
        setStoredVersion(p.version);
        setError(null);
      })
      .catch((e: Error) => {
        if (!cancelled) setError(e.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [layer]);

  useEffect(() => {
    if (layer !== "memory") return;
    let cancelled = false;
    setMemoryLoading(true);
    setMemoryError(null);
    if (memoryId) {
      void fetchMemory(memoryId)
        .then((m) => {
          if (!cancelled) setMemoryDetail(m);
        })
        .catch((e: Error) => {
          if (!cancelled) {
            setMemoryDetail(null);
            setMemoryError(e.message);
          }
        })
        .finally(() => {
          if (!cancelled) setMemoryLoading(false);
        });
    } else {
      void fetchMemories()
        .then((items) => {
          if (!cancelled) {
            setMemories(items);
            setMemoryDetail(null);
          }
        })
        .catch((e: Error) => {
          if (!cancelled) setMemoryError(e.message);
        })
        .finally(() => {
          if (!cancelled) setMemoryLoading(false);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [layer, memoryId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (layer === "memory") {
        if (memoryId) onNavigate({ layer: "memory", categoryId: "assistant", memoryId: null });
        else onNavigate({ layer: "assistant", categoryId: "assistant", memoryId: null });
      } else if (layer === "assistant") onNavigate({ layer: "categories", categoryId: null, memoryId: null });
      else onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [layer, memoryId, onClose, onNavigate]);

  const filtered = SETTINGS_CATEGORIES.filter(
    (c) => !search.trim() || c.title.includes(search.trim()) || c.id.includes(search.trim().toLowerCase()),
  );

  const save = () => {
    if (!draft || saving || injecting) return;
    setSaving(true);
    void saveProfile(draft, baseVersion)
      .then((result) => {
        if (!result.ok) {
          setError(result.reason);
          setProfile(result.current);
          setStoredVersion(result.current.version);
          // 冲突不覆盖用户正在编辑的草稿字段（DESIGN A05）
          return;
        }
        setProfile(result.profile);
        setDraft(result.profile);
        setBaseVersion(result.profile.version);
        setStoredVersion(result.profile.version);
        setError(null);
        setSavedFlash(true);
        onProfileSaved(result.profile);
        window.setTimeout(() => setSavedFlash(false), 1200);
      })
      .finally(() => setSaving(false));
  };

  return (
    <div className="desktop-panel settings-panel" data-testid="settings-panel" role="dialog" aria-label="设置与管理">
      <header className="desktop-panel-head">
        <button
          type="button"
          className="btn small"
          data-testid="settings-back"
          onClick={() => {
            if (layer === "memory") {
              if (memoryId) onNavigate({ layer: "memory", categoryId: "assistant", memoryId: null });
              else onNavigate({ layer: "assistant", categoryId: "assistant", memoryId: null });
            } else if (layer === "assistant") onNavigate({ layer: "categories", categoryId: null, memoryId: null });
            else onClose();
          }}
        >
          {layer === "categories" ? "关闭设置" : "返回上层"}
        </button>
        <h2>设置与管理</h2>
      </header>

      {layer === "categories" && (
        <div data-testid="settings-categories">
          <label className="settings-search">
            搜索
            <input
              data-testid="settings-search"
              aria-label="设置搜索"
              value={search}
              onChange={(e) =>
                onNavigate({ layer: "categories", categoryId: null, memoryId: null, search: e.target.value })
              }
              placeholder="助理 / 免打扰 / 记忆…"
            />
          </label>
          {filtered.length === 0 ? (
            <p className="desktop-panel-empty" data-testid="settings-search-empty">
              无匹配项，保留查询「{search}」
            </p>
          ) : (
            <ul className="settings-cat-list">
              {filtered.map((c) => (
                <li key={c.id}>
                  <button
                    type="button"
                    className="settings-cat"
                    data-testid={`settings-cat-${c.id}`}
                    onClick={() =>
                      onNavigate({
                        layer: c.id === "assistant" ? "assistant" : "categories",
                        categoryId: c.id,
                        memoryId: null,
                      })
                    }
                  >
                    {c.title}
                    {c.id !== "assistant" && c.id !== "connect" && <span className="muted">（占位）</span>}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {layer === "categories" && categoryId === "connect" && (
            <div data-testid="settings-connect">
              {!connectCardDismissed && connectCatalog && (
                <DiscoveryCard
                  catalog={connectCatalog}
                  onDismiss={() => {
                    writeAgentsIntroSeen();
                    setConnectCardDismissed(true);
                  }}
                  onManage={() => {
                    writeAgentsIntroSeen();
                    setConnectCardDismissed(true);
                  }}
                />
              )}
              <AgentsPanel />
            </div>
          )}
        </div>
      )}

      {layer === "assistant" && (
        <div data-testid="settings-assistant">
          <p className="settings-crumb" data-testid="settings-crumb">
            设置 → 我的 AI 助理
          </p>
          {loading || !draft || !profile ? (
            <p className="muted">加载配置…</p>
          ) : (
            <form
              className="settings-form"
              onSubmit={(e) => {
                e.preventDefault();
                save();
              }}
            >
              {(
                [
                  ["name", "名称"],
                  ["avatar", "头像"],
                  ["role", "工作定位"],
                  ["soul", "沟通方式"],
                  ["taste", "判断标准"],
                  ["memoryPrinciple", "记忆原则"],
                  ["proactivity", "主动性"],
                ] as const
              ).map(([key, label]) => (
                <label key={key} className="settings-field">
                  {label}
                  <input
                    data-testid={`profile-${key}`}
                    aria-label={label}
                    value={draft[key]}
                    onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
                  />
                </label>
              ))}
              <div className="settings-actions">
                <button
                  type="submit"
                  className="btn primary"
                  data-testid="profile-save"
                  disabled={saving || injecting || loading}
                >
                  保存
                </button>
                {testFixtures && (
                <button
                  type="button"
                  className="btn small"
                  data-testid="profile-inject-conflict"
                  disabled={injecting || saving}
                  onClick={() => {
                    setInjecting(true);
                    void bumpProfileVersionForConflict()
                      .then(async () => {
                        const p = await fetchProfile();
                        setStoredVersion(p.version);
                        setError(null);
                      })
                      .catch((e: Error) => setError(e.message))
                      .finally(() => setInjecting(false));
                  }}
                >
                  注入冲突（测试）
                </button>
                )}
                <button
                  type="button"
                  className="btn small"
                  data-testid="profile-open-memory"
                  onClick={() =>
                    onNavigate({ layer: "memory", categoryId: "assistant", memoryId: null })
                  }
                >
                  查看记忆
                </button>
              </div>
              {error && (
                <p className="evidence-error" data-testid="profile-save-error" role="alert">
                  {error}
                </p>
              )}
              {savedFlash && (
                <p className="settings-saved" data-testid="profile-saved" role="status">
                  已保存 · 版本 {profile.version}
                </p>
              )}
              <p className="muted" data-testid="profile-version">
                编辑基线版本 {baseVersion} · 当前存档版本 {storedVersion}
              </p>
            </form>
          )}
        </div>
      )}

      {layer === "memory" && (
        <div data-testid="settings-memory">
          <p className="settings-crumb" data-testid="memory-crumb">
            设置 → 我的 AI 助理 → 记忆
            {memoryId ? " → 详情" : ""}
          </p>
          {memoryLoading ? (
            <p className="muted">加载记忆…</p>
          ) : memoryError ? (
            <p className="evidence-error" role="alert" data-testid="memory-error">
              {memoryError}
            </p>
          ) : memoryId && memoryDetail ? (
            <MemoryDetail
              entry={memoryDetail}
              onSaved={(m) => setMemoryDetail(m)}
              onWithdrawn={() =>
                onNavigate({ layer: "memory", categoryId: "assistant", memoryId: null })
              }
              onBack={() => onNavigate({ layer: "memory", categoryId: "assistant", memoryId: null })}
            />
          ) : (
            <MemoryList
              items={memories}
              onOpen={(id) => onNavigate({ layer: "memory", categoryId: "assistant", memoryId: id })}
              onBack={() => onNavigate({ layer: "assistant", categoryId: "assistant", memoryId: null })}
            />
          )}
        </div>
      )}
    </div>
  );
}

function MemoryList({
  items,
  onOpen,
  onBack,
}: {
  items: MemoryEntry[];
  onOpen: (id: string) => void;
  onBack: () => void;
}) {
  return (
    <div data-testid="memory-list">
      {items.length === 0 ? (
        <p className="desktop-panel-empty">暂无记忆条目</p>
      ) : (
        <ul className="settings-cat-list">
          {items.map((m) => (
            <li key={m.id}>
              <button
                type="button"
                className="settings-cat"
                data-testid={`memory-item-${m.id}`}
                onClick={() => onOpen(m.id)}
              >
                {m.title}
              </button>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className="btn small" data-testid="memory-back" onClick={onBack}>
        返回助理分类
      </button>
    </div>
  );
}

function MemoryDetail({
  entry,
  onSaved,
  onWithdrawn,
  onBack,
}: {
  entry: MemoryEntry;
  onSaved: (m: MemoryEntry) => void;
  onWithdrawn: () => void;
  onBack: () => void;
}) {
  const [draftBody, setDraftBody] = useState(entry.body);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    setDraftBody(entry.body);
    setErr(null);
  }, [entry.id, entry.body]);

  useEffect(() => {
    setMsg(null);
  }, [entry.id]);

  return (
    <div data-testid="memory-detail">
      <h3>{entry.title}</h3>
      <dl className="evidence-dl">
        <dt>来源</dt>
        <dd data-testid="memory-source">{entry.source}</dd>
        <dt>有效期</dt>
        <dd data-testid="memory-expires">{entry.expires}</dd>
      </dl>
      <label className="settings-field">
        正文（可纠错）
        <textarea
          data-testid="memory-body-input"
          aria-label="记忆正文"
          rows={3}
          value={draftBody}
          onChange={(e) => setDraftBody(e.target.value)}
        />
      </label>
      <p className="muted" data-testid="memory-body">
        当前存档：{entry.body}
      </p>
      <div className="settings-actions">
        <button
          type="button"
          className="btn primary"
          data-testid="memory-correct-save"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            setErr(null);
            void correctMemory(entry.id, { body: draftBody })
              .then((m) => {
                onSaved(m);
                setMsg("已纠错保存");
              })
              .catch((e: Error) => setErr(e.message))
              .finally(() => setBusy(false));
          }}
        >
          保存纠错
        </button>
        <button
          type="button"
          className="btn small"
          data-testid="memory-withdraw"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            setErr(null);
            void withdrawMemory(entry.id)
              .then(() => onWithdrawn())
              .catch((e: Error) => setErr(e.message))
              .finally(() => setBusy(false));
          }}
        >
          撤回此记忆
        </button>
        <button type="button" className="btn small" data-testid="memory-detail-back" onClick={onBack}>
          返回记忆列表
        </button>
      </div>
      {msg && (
        <p className="settings-saved" data-testid="memory-correct-saved" role="status">
          {msg}
        </p>
      )}
      {err && (
        <p className="evidence-error" data-testid="memory-write-error" role="alert">
          {err}
        </p>
      )}
    </div>
  );
}
