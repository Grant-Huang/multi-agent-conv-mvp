//! MCP gateway: configured trust, discovered schemas, bounded calls, no business retries.
use crate::{
    capability::{CapabilityCard, Effect},
    tools::{Tool, ToolOutput, ToolRegistry},
};
use an_core::Artifact;
use anyhow::{bail, Context, Result};
use rmcp::{
    model::{CallToolRequestParams, PaginatedRequestParams},
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::{process::Command, sync::Mutex, time::timeout};

struct Session {
    service: RunningService<RoleClient, ()>,
    _child: Option<ChildGuard>,
}
impl std::ops::Deref for Session {
    type Target = RunningService<RoleClient, ()>;
    fn deref(&self) -> &Self::Target {
        &self.service
    }
}
struct ChildGuard(Option<tokio::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.start_kill();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = child.wait().await;
                });
            }
        }
    }
}
const MAX_TOOLS: usize = 64;
const MAX_RESULT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Serialize)]
pub struct McpServerConfig {
    pub id: String,
    #[serde(flatten)]
    pub transport: McpTransport,
    pub timeout_ms: u64,
    pub tools: Vec<ToolBinding>,
}
impl<'de> Deserialize<'de> for McpServerConfig {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let value = Value::deserialize(de)?;
        let map = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("MCP server must be an object"))?;
        let transport_keys: &[&str] = match map.get("transport").and_then(Value::as_str) {
            Some("stdio") => &["command", "args", "cwd"],
            Some("http") => &["url", "bearer_env"],
            _ => return Err(serde::de::Error::custom("unknown MCP transport")),
        };
        if map.keys().any(|k| {
            !["id", "transport", "timeout_ms", "tools"].contains(&k.as_str())
                && !transport_keys.contains(&k.as_str())
        }) {
            return Err(serde::de::Error::custom("unknown MCP server field"));
        }
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            #[serde(flatten)]
            transport: McpTransport,
            #[serde(default = "default_timeout")]
            timeout_ms: u64,
            tools: Vec<ToolBinding>,
        }
        let wire: Wire = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self {
            id: wire.id,
            transport: wire.transport,
            timeout_ms: wire.timeout_ms,
            tools: wire.tools,
        })
    }
}
fn default_timeout() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        bearer_env: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBinding {
    pub name: String,
    pub capability_id: String,
    pub description: String,
    pub effect: Effect,
    #[serde(default)]
    pub input_types: Vec<String>,
    #[serde(default)]
    pub output_types: Vec<String>,
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}

fn compile_schema(schema: &Value) -> Result<jsonschema::Validator> {
    fn check(value: &Value) -> bool {
        match value {
            Value::Object(map) => map.iter().all(|(key, value)| {
                if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef")
                    && !value.as_str().is_some_and(|s| s.starts_with('#'))
                {
                    return false;
                }
                check(value)
            }),
            Value::Array(items) => items.iter().all(check),
            _ => true,
        }
    }
    if schema.to_string().len() > 32_768 || !check(schema) {
        bail!("E_MCP_SCHEMA: external references or oversized schema");
    }
    jsonschema::validator_for(schema).map_err(|_| anyhow::anyhow!("E_MCP_SCHEMA: invalid schema"))
}

async fn open(cfg: &McpServerConfig) -> Result<Session> {
    match &cfg.transport {
        McpTransport::Stdio { command, args, cwd } => {
            if command.is_empty() {
                bail!("E_MCP_CONFIG: empty command");
            }
            let mut cmd = Command::new(command);
            cmd.args(args)
                .kill_on_drop(true)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            if let Some(cwd) = cwd {
                cmd.current_dir(cwd);
            }
            let mut child = ChildGuard(Some(
                cmd.spawn().map_err(|_| anyhow::anyhow!("E_MCP_CONNECT"))?,
            ));
            let process = child.0.as_mut().expect("owned child");
            let pipes = (
                process.stdout.take().context("E_MCP_CONNECT")?,
                process.stdin.take().context("E_MCP_CONNECT")?,
            );
            let transport = rmcp::transport::async_rw::AsyncRwTransport::new(pipes.0, pipes.1)
                .with_max_line_length(MAX_RESULT_BYTES);
            let service = ().serve(transport).await.map_err(|_| anyhow::anyhow!("E_MCP_CONNECT"))?;
            Ok(Session {
                service,
                _child: Some(child),
            })
        }
        McpTransport::Http { url, bearer_env } => {
            let parsed = reqwest_mcp::Url::parse(url)
                .map_err(|_| anyhow::anyhow!("E_MCP_CONFIG: invalid URL"))?;
            let local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
            if !(parsed.scheme() == "https" || parsed.scheme() == "http" && local)
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                bail!("E_MCP_CONFIG: HTTPS required; no URL credentials, query or fragment");
            }
            let client = reqwest_mcp::Client::builder()
                .redirect(reqwest_mcp::redirect::Policy::none())
                .build()
                .map_err(|_| anyhow::anyhow!("E_MCP_CONNECT"))?;
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone());
            config.reinit_on_expired_session = false;
            config.max_sse_event_size = MAX_RESULT_BYTES;
            if let Some(name) = bearer_env {
                config.auth_header = Some(
                    std::env::var(name)
                        .map_err(|_| anyhow::anyhow!("E_MCP_AUTH: missing binding"))?,
                );
            }
            let service =
                ().serve(StreamableHttpClientTransport::with_client(client, config))
                    .await
                    .map_err(|_| anyhow::anyhow!("E_MCP_CONNECT"))?;
            Ok(Session {
                service,
                _child: None,
            })
        }
    }
}

/// Discover a fixed startup snapshot. No server-provided tool is authorized implicitly.
pub async fn connect(configs: &[McpServerConfig]) -> Result<ToolRegistry> {
    let mut ids = BTreeSet::new();
    if configs.len() > 8 || configs.iter().map(|c| c.tools.len()).sum::<usize>() > MAX_TOOLS {
        bail!("E_MCP_CONFIG: at most eight servers and 64 bindings");
    }
    for cfg in configs {
        if !identifier(&cfg.id)
            || !ids.insert(cfg.id.clone())
            || cfg.tools.is_empty()
            || cfg.tools.len() > MAX_TOOLS
            || !(1..=300_000).contains(&cfg.timeout_ms)
        {
            bail!("E_MCP_CONFIG: invalid or duplicate server, bindings or timeout");
        }
        let mut names = BTreeSet::new();
        for binding in &cfg.tools {
            if !identifier(&binding.name)
                || !names.insert(binding.name.clone())
                || binding.capability_id.is_empty()
                || binding.capability_id.len() > 128
                || binding.description.is_empty()
                || binding.description.len() > 2048
            {
                bail!("E_MCP_CONFIG: invalid or duplicate binding");
            }
        }
    }
    let mut registry = ToolRegistry::default();
    for cfg in configs {
        let duration = Duration::from_millis(cfg.timeout_ms);
        let result = timeout(duration, async {
            let session = open(cfg).await?;
            let mut discovered = BTreeMap::new();
            let mut cursor = None;
            let mut seen = BTreeSet::new();
            loop {
                let page = session
                    .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                    .await
                    .map_err(|_| anyhow::anyhow!("E_MCP_DISCOVERY"))?;
                for tool in page.tools {
                    if discovered.insert(tool.name.to_string(), tool).is_some()
                        || discovered.len() > 512
                    {
                        bail!("E_MCP_DISCOVERY: duplicate or oversized directory");
                    }
                }
                cursor = page.next_cursor;
                if let Some(c) = &cursor {
                    if !seen.insert(c.clone()) || seen.len() > 32 {
                        bail!("E_MCP_DISCOVERY: invalid pagination");
                    }
                } else {
                    break;
                }
            }
            let connection = Arc::new(Mutex::new(Some(session)));
            let mut tools = ToolRegistry::default();
            for binding in &cfg.tools {
                let remote = discovered
                    .get(&binding.name)
                    .context("E_MCP_DISCOVERY: configured tool missing")?;
                let input_schema = Value::Object((*remote.input_schema).clone());
                let output_schema = remote
                    .output_schema
                    .as_ref()
                    .map(|s| Value::Object((**s).clone()));
                let input = compile_schema(&input_schema)?;
                let output = output_schema.as_ref().map(compile_schema).transpose()?;
                let card = CapabilityCard {
                    capability_id: binding.capability_id.clone(),
                    tool_id: format!("{}.{}", cfg.id, binding.name),
                    description: binding.description.clone(),
                    provider: cfg.id.clone(),
                    effect: binding.effect,
                    input_schema,
                    output_schema,
                    input_types: binding.input_types.clone(),
                    output_types: binding.output_types.clone(),
                };
                let hint = card.input_schema.to_string();
                tools.register(Arc::new(McpTool {
                    card,
                    hint,
                    remote_name: binding.name.clone(),
                    input,
                    output,
                    connection: connection.clone(),
                    duration,
                }));
            }
            Ok::<_, anyhow::Error>(tools)
        })
        .await
        .map_err(|_| anyhow::anyhow!("E_MCP_TIMEOUT: discovery {}", cfg.id))?
        .with_context(|| format!("MCP server {}", cfg.id))?;
        registry.extend(result)?;
    }
    Ok(registry)
}

struct McpTool {
    card: CapabilityCard,
    hint: String,
    remote_name: String,
    input: jsonschema::Validator,
    output: Option<jsonschema::Validator>,
    connection: Arc<Mutex<Option<Session>>>,
    duration: Duration,
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.card.tool_id
    }
    fn description(&self) -> &str {
        &self.card.description
    }
    fn args_hint(&self) -> &str {
        &self.hint
    }
    fn capability(&self) -> CapabilityCard {
        self.card.clone()
    }
    async fn call(&self, args: &Value) -> Result<ToolOutput> {
        if matches!(
            self.card.effect,
            Effect::ExternalWrite | Effect::Destructive
        ) {
            bail!("E_MCP_DENIED");
        }
        if !args.is_object()
            || args.to_string().len() > MAX_RESULT_BYTES
            || !self.input.is_valid(args)
        {
            bail!("E_MCP_ARGUMENTS");
        }
        timeout(self.duration, async {
            let mut slot = self.connection.lock().await;
            // Taking ownership ensures cancellation drops the session rather than reusing a dirty transport.
            let session = slot
                .take()
                .context("E_MCP_UNAVAILABLE: restart to rediscover")?;
            if session.is_closed() {
                bail!("E_MCP_UNAVAILABLE");
            }
            let result = session
                .peer()
                .call_tool(
                    CallToolRequestParams::new(self.remote_name.clone())
                        .with_arguments(args.as_object().expect("checked object").clone()),
                )
                .await
                .map_err(|_| anyhow::anyhow!("E_MCP_PROTOCOL: outcome may be unknown"))?;
            *slot = Some(session);
            if result.is_error == Some(true) {
                bail!("E_MCP_TOOL");
            }
            if let Some(schema) = &self.output {
                if !result
                    .structured_content
                    .as_ref()
                    .is_some_and(|v| schema.is_valid(v))
                {
                    bail!("E_MCP_OUTPUT");
                }
            }
            let data = serde_json::to_value(result)?;
            if data.get("resultType").is_some_and(|v| v != "complete") {
                bail!("E_MCP_OUTPUT: interactive or deferred results are not supported");
            }
            if data.to_string().len() > MAX_RESULT_BYTES {
                bail!("E_MCP_OUTPUT: result too large");
            }
            let mut artifacts = vec![];
            if let Some(contents) = data["content"].as_array() {
                for item in contents {
                    if item["type"] == "resource" {
                        if let Some(text) = item["resource"]["text"].as_str() {
                            artifacts.push(Artifact {
                                kind: "text".into(),
                                name: format!("{}-resource-{}", self.name(), artifacts.len() + 1),
                                content: text.into(),
                            });
                        }
                    }
                }
            }
            Ok(ToolOutput {
                text: json!({"status":"success","data":data,"artifacts":artifacts,"error":null})
                    .to_string(),
                artifacts,
            })
        })
        .await
        .map_err(|_| anyhow::anyhow!("E_MCP_TIMEOUT: outcome may be unknown; not retried"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schemas_cannot_fetch_external_references() {
        for reference in [
            "https://example.com/schema",
            "file:///etc/passwd",
            "other.json",
        ] {
            assert!(compile_schema(&json!({"$ref":reference})).is_err());
        }
        assert!(
            compile_schema(&json!({"$defs":{"s":{"type":"string"}},"$ref":"#/$defs/s"}))
                .unwrap()
                .is_valid(&json!("ok"))
        );
        assert!(compile_schema(&json!({"type":"nonsense"})).is_err());
    }
    #[test]
    fn config_rejects_unknown_fields_and_requires_explicit_effect() {
        let base = json!({"id":"s","transport":"stdio","command":"python3", "tools":[{
            "name":"echo","capability_id":"test.echo","description":"Echo","effect":"read_only"}]});
        assert!(serde_json::from_value::<McpServerConfig>(base.clone()).is_ok());
        let mut unknown = base.clone();
        unknown["timeot_ms"] = json!(10);
        assert!(serde_json::from_value::<McpServerConfig>(unknown).is_err());
        let mut missing = base;
        missing["tools"][0]
            .as_object_mut()
            .unwrap()
            .remove("effect");
        assert!(serde_json::from_value::<McpServerConfig>(missing).is_err());
    }
}
