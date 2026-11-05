// 节点页：局域网发现的节点（输入配对码即可配对）、按地址手动添加、已配对节点的状态。
import { useEffect, useState } from "react";
import { api, type NodesView } from "../api";

export function NodesPanel() {
  const [view, setView] = useState<NodesView>({ discovered: [], paired: [] });
  const [error, setError] = useState<string | null>(null);

  const load = () => api.nodes().then(setView).catch((e) => setError(String(e)));

  // 打开页面时加载，之后每 5 秒刷新一次（节点上线 / 离线 / 新发现）
  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);

  const pair = async (address: string, code: string) => {
    setError(null);
    try {
      await api.pairNode(address, code);
      await load();
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  };

  const unpair = async (id: string, name: string) => {
    if (!confirm(`解除与「${name}」的配对？它上面的 agent 将不再可用。`)) return;
    await api.unpairNode(id).catch((e) => setError(String(e)));
    await load();
  };

  const unpaired = view.discovered.filter((d) => !d.paired);

  return (
    <div className="task-list">
      {error && <div className="kv bad-text">{error}</div>}

      <section className="section">
        <h4>已配对的节点</h4>
        {view.paired.length === 0 && <div className="kv muted">还没有。在另一台电脑上运行 an-node，然后在下面配对。</div>}
        {view.paired.map((n) => (
          <div key={n.node_id} className="task-card log">
            <div className="row">
              <span className={`dot ${n.online ? "ok" : "bad"}`} />
              <b>{n.name}</b>
              <span className="kv muted">{n.address}</span>
              <button className="btn small push-right" onClick={() => unpair(n.node_id, n.name)}>解除配对</button>
            </div>
            <div className="kv">agent：{n.agents.length > 0 ? n.agents.join("、") : "无"}</div>
            {!n.online && n.last_error && <div className="kv bad-text">{n.last_error}</div>}
          </div>
        ))}
      </section>

      <section className="section">
        <h4>局域网中发现的节点</h4>
        {unpaired.length === 0 && <div className="kv muted">没有发现待配对的节点。网络屏蔽组播时，请用下面的「按地址添加」。</div>}
        {unpaired.map((d) => (
          <PairForm key={d.node_id} title={`${d.name}（${d.os}）`} fixedAddress={d.address} onPair={pair} />
        ))}
      </section>

      <section className="section">
        <h4>按地址添加</h4>
        <PairForm title="" onPair={pair} />
      </section>
    </div>
  );
}

/** 配对表单：地址（发现的节点已知）+ 节点上显示的 6 位配对码 */
function PairForm(props: { title: string; fixedAddress?: string; onPair: (address: string, code: string) => Promise<boolean> }) {
  const [address, setAddress] = useState(props.fixedAddress ?? "");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    const ok = await props.onPair(address, code.replace(/\s/g, ""));
    setBusy(false);
    if (ok) setCode("");
  };
  return (
    <div className="task-card log">
      {props.title && (
        <div className="row">
          <b>{props.title}</b>
          <span className="kv muted">{props.fixedAddress}</span>
        </div>
      )}
      <div className="row">
        {!props.fixedAddress && (
          <input className="input" placeholder="地址，如 192.168.1.23" value={address} onChange={(e) => setAddress(e.target.value)} />
        )}
        <input className="input code" placeholder="配对码" value={code} maxLength={7} onChange={(e) => setCode(e.target.value)} />
        <button className="btn primary small" disabled={busy || !address.trim() || code.replace(/\s/g, "").length !== 6} onClick={submit}>
          配对
        </button>
      </div>
    </div>
  );
}
