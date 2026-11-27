//! 连接器注册表（契约 [connector.md](../../docs/contracts/connector.md) K5/K6；#487）。
//!
//! 外部系统统一在 `connectors.toml` 注册（描述符见契约 §5 最小字段示例）：
//! 发现 / 健康 / 凭据引用（K2，env 名而非明文）一处可查。业务代码按 `id` 查找，
//! **未注册的外部系统在迁移期只告警不拒绝**（用户 2026-11-27 拍板：先告警后拒绝）。
//!
//! - 文件缺失视为空注册表（不报错——所有面还没迁移时不告警噪音）。
//! - `credentials.env` 在查找时解析为实际值；env 未设置返回 `Lookup::Found` 但
//!   `credential: None`，由调用方决定是否需要凭据。

use anyhow::Context as _;
use serde::Deserialize;
use std::path::Path;

/// 单个连接器描述符（契约 §5 最小字段）。
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectorDescriptor {
    pub id: String,
    /// business | llm | agent | tool
    pub kind: String,
    /// http | stdio | acp | a2a
    pub transport: String,
    pub endpoint: String,
    /// K2 凭据引用：指向存放密钥的环境变量名（不存明文）
    #[serde(default)]
    pub credentials: Option<CredentialRef>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub health: Option<HealthProbe>,
    #[serde(default)]
    pub owner: Option<String>,
}

/// K2 凭据引用：环境变量名。
#[derive(Debug, Clone, Deserialize)]
pub struct CredentialRef {
    pub env: String,
}

/// 健康探测声明。
#[derive(Debug, Clone, Deserialize)]
pub struct HealthProbe {
    pub probe: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
}

fn default_interval() -> u64 {
    300
}

/// 注册表：`connectors.toml` 的解析结果。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub connectors: Vec<ConnectorDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct File {
    #[serde(default, rename = "connector")]
    connectors: Vec<ConnectorDescriptor>,
}

/// 从指定路径加载注册表。文件缺失 → 空注册表（Ok）；
/// 文件存在但解析失败 → Err（配置错误要显式暴露）。
pub fn load(path: &Path) -> anyhow::Result<Registry> {
    if !path.exists() {
        return Ok(Registry::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取连接器注册表失败：{}", path.display()))?;
    let file: File = toml::from_str(&raw)
        .with_context(|| format!("解析连接器注册表失败：{}", path.display()))?;
    Ok(Registry {
        connectors: file.connectors,
    })
}

impl Registry {
    /// 按 id 查找。未命中返回 None——调用方按拍板②走「告警 + 迁移期回退」。
    pub fn lookup(&self, id: &str) -> Option<&ConnectorDescriptor> {
        self.connectors.iter().find(|c| c.id == id)
    }
}

/// K2 凭据引用解析：读 `credentials.env` 指向的环境变量。
/// 返回 None = 未配置凭据（服务不需要鉴权）或 env 未设置（告警由调用方记）。
pub fn resolve_credential(desc: &ConnectorDescriptor) -> Option<String> {
    let env = desc.credentials.as_ref()?.env.as_str();
    match std::env::var(env) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_parses_sample_and_lookup_hits() {
        let dir = std::env::temp_dir().join(an_core::new_id("conn"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("connectors.toml");
        std::fs::write(
            &path,
            r#"
[[connector]]
id = "relos"
kind = "business"
transport = "http"
endpoint = "http://127.0.0.1:8000"
credentials = { env = "RELOS_TOKEN" }
capabilities = ["context.compile"]
health = { probe = "GET /v1/health", interval_secs = 300 }
owner = "line5"
"#,
        )
        .unwrap();
        let reg = load(&path).unwrap();
        let relos = reg.lookup("relos").expect("应命中 relos");
        assert_eq!(relos.endpoint, "http://127.0.0.1:8000");
        assert_eq!(relos.credentials.as_ref().unwrap().env, "RELOS_TOKEN");
        assert!(reg.lookup("nope").is_none());
    }

    #[test]
    fn missing_file_is_empty_registry() {
        let reg = load(Path::new("/nonexistent-dir-zz/connectors.toml")).unwrap();
        assert!(reg.connectors.is_empty());
        assert!(reg.lookup("relos").is_none());
    }

    #[test]
    fn broken_toml_is_err() {
        let dir = std::env::temp_dir().join(an_core::new_id("conn-bad"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.toml");
        std::fs::write(&path, "[[connector]]\nid = ").unwrap();
        assert!(load(&path).is_err());
    }
}
