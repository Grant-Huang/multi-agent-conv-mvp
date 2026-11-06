//! 节点管理（AgentNexus 这一侧）。
//!
//! 节点 = 运行在另一台机器（或本机）上的 `an-node` 程序，它在那台机器上执行 agent。
//! 这里负责：
//! 1. **发现**：用 mDNS 在局域网里自动找到节点（跨平台，不依赖 macOS 的 Bonjour）；
//!    网络屏蔽组播时，也可以按地址手动添加；
//! 2. **配对**：用户输入节点上显示的 6 位配对码，换取长期令牌，保存在数据库；
//! 3. **心跳**：定期检查节点是否在线，同步它上面的 agent 列表到注册表；
//!    节点离线时它的 agent 标记为不可用（执行组会跳过它们），恢复后自动可用。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use an_agents::{NodeClient, NodeConn, RemoteAgent};
use an_core::node::SERVICE_TYPE;
use an_core::{codes, AgentAdapter, AgentCard, AgentError};
use serde::Serialize;
use ts_rs::TS;

use crate::registry::AgentRegistry;
use crate::store::{Store, StoredNode};

/// 局域网中发现的节点
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DiscoveredNode {
    pub node_id: String,
    pub name: String,
    pub address: String,
    pub os: String,
    /// 是否已经配对
    pub paired: bool,
}

/// 已配对节点的状态
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PairedNode {
    pub node_id: String,
    pub name: String,
    pub address: String,
    pub online: bool,
    /// 最近一次连接失败的原因
    pub last_error: Option<String>,
    /// 节点上的 agent（在 AgentNexus 中的 id）
    pub agents: Vec<String>,
    pub paired_at: String,
}

pub struct NodeManager {
    store: Arc<Store>,
    registry: Arc<AgentRegistry>,
    client: NodeClient,
    /// 配对时告诉节点"我是谁"
    client_name: String,
    discovered: Mutex<BTreeMap<String, DiscoveredNode>>,
    /// node_id → (是否在线, 最近错误)
    status: Mutex<BTreeMap<String, (bool, Option<String>)>>,
}

impl NodeManager {
    pub fn new(store: Arc<Store>, registry: Arc<AgentRegistry>, client_name: String) -> Arc<Self> {
        Arc::new(Self {
            store,
            registry,
            client: NodeClient::default(),
            client_name,
            discovered: Mutex::default(),
            status: Mutex::default(),
        })
    }

    fn conn_of(n: &StoredNode) -> NodeConn {
        NodeConn {
            node_id: n.node_id.clone(),
            name: n.name.clone(),
            address: n.address.clone(),
            token: n.token.clone(),
        }
    }

    /// 把节点的 agent 写入注册表
    fn publish_agents(&self, n: &StoredNode, cards: &[AgentCard], online: bool) {
        let agents: Vec<Arc<dyn AgentAdapter>> = cards
            .iter()
            .map(|c| {
                Arc::new(RemoteAgent::new(
                    self.client.clone(),
                    Self::conn_of(n),
                    c.clone(),
                    online,
                )) as Arc<dyn AgentAdapter>
            })
            .collect();
        self.registry.replace_node_agents(&n.node_id, agents);
    }

    /// 启动时调用：先用数据库里的信息注册（标记为离线），随后由心跳更新为真实状态
    pub fn load_saved(&self) -> anyhow::Result<()> {
        for n in self.store.list_nodes()? {
            self.publish_agents(&n, &n.cards, false);
            self.status
                .lock()
                .unwrap()
                .insert(n.node_id.clone(), (false, Some("尚未连接".into())));
        }
        Ok(())
    }

    /// 检查一个节点：在线则同步 agent 列表，离线则标记不可用。只在状态变化时写日志，避免刷屏
    pub async fn refresh(&self, n: &StoredNode) {
        let result = self.client.agents(&Self::conn_of(n)).await;
        let was_online = self
            .status
            .lock()
            .unwrap()
            .get(&n.node_id)
            .map(|s| s.0)
            .unwrap_or(false);
        match result {
            Ok(cards) => {
                if !was_online {
                    tracing::info!(event = "node.online", node_id = %n.node_id, name = %n.name, agents = cards.len(), "节点上线");
                }
                let _ = self.store.update_node_cards(&n.node_id, &cards);
                self.publish_agents(n, &cards, true);
                self.status
                    .lock()
                    .unwrap()
                    .insert(n.node_id.clone(), (true, None));
            }
            Err(e) => {
                if was_online {
                    tracing::warn!(event = "node.offline", code = codes::NODE_OFFLINE, node_id = %n.node_id, name = %n.name, error = %e, "节点离线");
                }
                if e.code == codes::NODE_AUTH {
                    tracing::error!(event = "node.auth_failed", code = codes::NODE_AUTH, node_id = %n.node_id, "节点拒绝了令牌，需要重新配对");
                }
                self.publish_agents(n, &n.cards, false);
                self.status
                    .lock()
                    .unwrap()
                    .insert(n.node_id.clone(), (false, Some(e.to_string())));
            }
        }
    }

    pub async fn refresh_all(&self) {
        for n in self.store.list_nodes().unwrap_or_default() {
            // 重新读一次，拿到最新保存的 agent 列表
            self.refresh(&n).await;
        }
    }

    /// 配对：确认节点身份 → 用配对码换令牌 → 保存 → 读取 agent 列表
    pub async fn pair(&self, address: &str, code: &str) -> Result<PairedNode, AgentError> {
        let address = normalize_address(address);
        let info = self.client.info(&address).await?;
        if info.protocol != an_core::node::PROTOCOL_VERSION {
            return Err(AgentError::new(
                codes::NODE_PROTOCOL,
                format!(
                    "节点协议版本 {} 与本机 {} 不一致，请升级",
                    info.protocol,
                    an_core::node::PROTOCOL_VERSION
                ),
            ));
        }
        let resp = self.client.pair(&address, code, &self.client_name).await.inspect_err(|e| {
            tracing::warn!(event = "node.pair_failed", code = e.code, address = %address, error = %e.message, "配对失败");
        })?;
        let mut stored = StoredNode {
            node_id: resp.node_id.clone(),
            name: resp.name.clone(),
            address: address.clone(),
            token: resp.token,
            cards: vec![],
            paired_at: an_core::now(),
        };
        stored.cards = self.client.agents(&Self::conn_of(&stored)).await?;
        self.store
            .upsert_node(&stored)
            .map_err(|e| AgentError::new(codes::NODE_PROTOCOL, format!("保存节点失败：{e:#}")))?;
        self.publish_agents(&stored, &stored.cards, true);
        self.status
            .lock()
            .unwrap()
            .insert(stored.node_id.clone(), (true, None));
        if let Some(d) = self.discovered.lock().unwrap().get_mut(&stored.node_id) {
            d.paired = true;
        }
        tracing::info!(event = "node.paired", node_id = %stored.node_id, name = %stored.name, address = %address, agents = stored.cards.len(), "节点配对成功");
        Ok(self
            .paired()
            .into_iter()
            .find(|p| p.node_id == stored.node_id)
            .expect("刚保存的节点"))
    }

    /// 解除配对：删除令牌，移除它的 agent
    pub fn unpair(&self, node_id: &str) -> anyhow::Result<bool> {
        let removed = self.store.delete_node(node_id)?;
        self.registry.remove_node(node_id);
        self.status.lock().unwrap().remove(node_id);
        if let Some(d) = self.discovered.lock().unwrap().get_mut(node_id) {
            d.paired = false;
        }
        if removed {
            tracing::info!(event = "node.unpaired", node_id = %node_id, "已解除节点配对");
        }
        Ok(removed)
    }

    /// 已配对的节点及其状态
    pub fn paired(&self) -> Vec<PairedNode> {
        let status = self.status.lock().unwrap().clone();
        let cards = self.registry.cards();
        self.store
            .list_nodes()
            .unwrap_or_default()
            .into_iter()
            .map(|n| {
                let (online, last_error) = status.get(&n.node_id).cloned().unwrap_or((false, None));
                let agents = cards
                    .iter()
                    .filter(|c| c.kind == "node" && c.location == n.name)
                    .map(|c| c.id.clone())
                    .collect();
                PairedNode {
                    node_id: n.node_id,
                    name: n.name,
                    address: n.address,
                    online,
                    last_error,
                    agents,
                    paired_at: n.paired_at,
                }
            })
            .collect()
    }

    /// 局域网中发现的节点
    pub fn discovered(&self) -> Vec<DiscoveredNode> {
        self.discovered.lock().unwrap().values().cloned().collect()
    }

    /// 后台心跳：定期检查所有已配对节点
    pub fn start_heartbeat(self: &Arc<Self>, every: Duration) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                this.refresh_all().await;
                tokio::time::sleep(every).await;
            }
        });
    }

    /// 后台发现：浏览局域网中的 `_agentnexus._tcp` 服务
    pub fn start_discovery(self: &Arc<Self>) {
        let daemon = match mdns_sd::ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(event = "discovery.unavailable", code = codes::DISCOVERY, error = %e, "无法启动局域网发现，可以在界面上按地址手动添加节点");
                return;
            }
        };
        let receiver = match daemon.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(event = "discovery.unavailable", code = codes::DISCOVERY, error = %e, "无法启动局域网发现");
                return;
            }
        };
        let this = self.clone();
        tokio::spawn(async move {
            let _daemon = daemon; // 保持守护进程存活
                                  // fullname → node_id，用于处理"服务消失"事件
            let mut names: BTreeMap<String, String> = BTreeMap::new();
            while let Ok(event) = receiver.recv_async().await {
                match event {
                    mdns_sd::ServiceEvent::ServiceResolved(info) => {
                        let Some(node_id) = info.get_property_val_str("id").map(String::from)
                        else {
                            continue;
                        };
                        // 优先使用 IPv4 地址；mDNS 解析出的主机名在部分系统上不可用
                        let mut v4: Vec<_> = info.get_addresses_v4().into_iter().collect();
                        v4.sort();
                        let host = v4
                            .iter()
                            .find(|ip| !ip.is_loopback())
                            .or(v4.first())
                            .map(|ip| ip.to_string())
                            .unwrap_or_else(|| {
                                info.get_hostname().trim_end_matches('.').to_string()
                            });
                        let paired = this
                            .store
                            .list_nodes()
                            .unwrap_or_default()
                            .iter()
                            .any(|n| n.node_id == node_id);
                        let node = DiscoveredNode {
                            node_id: node_id.clone(),
                            name: info
                                .get_property_val_str("name")
                                .unwrap_or("未命名节点")
                                .to_string(),
                            address: format!("http://{host}:{}", info.get_port()),
                            os: info.get_property_val_str("os").unwrap_or("").to_string(),
                            paired,
                        };
                        names.insert(info.get_fullname().to_string(), node_id.clone());
                        let is_new = this
                            .discovered
                            .lock()
                            .unwrap()
                            .insert(node_id.clone(), node.clone())
                            .is_none();
                        if is_new {
                            tracing::info!(event = "discovery.found", node_id = %node_id, name = %node.name, address = %node.address, "发现局域网节点");
                        }
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                        if let Some(id) = names.remove(&fullname) {
                            this.discovered.lock().unwrap().remove(&id);
                            tracing::info!(event = "discovery.removed", node_id = %id, "局域网节点已离开");
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

/// 补全地址：`192.168.1.23` → `http://192.168.1.23:7788`
pub fn normalize_address(input: &str) -> String {
    let s = input.trim().trim_end_matches('/');
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    // 没写端口时补上默认端口
    let after_scheme = with_scheme.split("://").nth(1).unwrap_or("");
    if after_scheme.contains(':') {
        with_scheme
    } else {
        format!("{with_scheme}:{}", DEFAULT_NODE_PORT)
    }
}

/// 节点默认端口
pub const DEFAULT_NODE_PORT: u16 = 7788;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_normalization() {
        assert_eq!(
            normalize_address("192.168.1.23"),
            "http://192.168.1.23:7788"
        );
        assert_eq!(
            normalize_address(" 192.168.1.23:9000/ "),
            "http://192.168.1.23:9000"
        );
        assert_eq!(
            normalize_address("http://mac.local"),
            "http://mac.local:7788"
        );
        assert_eq!(
            normalize_address("https://n.example:443"),
            "https://n.example:443"
        );
    }
}
