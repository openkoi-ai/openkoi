// src/plugins/mcp.rs — MCP tool servers (subprocess, JSON-RPC over stdio)

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::infra::config::McpServerConfig;
use crate::provider::ToolDef;

/// A single MCP tool server subprocess.
pub struct McpToolServer {
    pub name: String,
    pub tools: Vec<McpTool>,
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// A tool exposed by an MCP server.
#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpTool {
    /// Convert to a ToolDef with namespaced name (server__tool).
    pub fn to_tool_def(&self, server_name: &str) -> ToolDef {
        ToolDef {
            name: format!("{server_name}__{}", self.name),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }
}

/// Manages all MCP tool servers.
pub struct McpManager {
    servers: HashMap<String, McpToolServer>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Returns true if any servers are connected.
    pub fn has_servers(&self) -> bool {
        !self.servers.is_empty()
    }

    /// Start all configured servers. Called once per session.
    pub async fn start_all(&mut self, configs: &[McpServerConfig]) -> Result<()> {
        for cfg in configs {
            match McpToolServer::spawn(cfg).await {
                Ok(mut server) => match server.initialize().await {
                    Ok(tools) => {
                        tracing::info!(
                            "MCP server '{}': {} tools available",
                            cfg.name,
                            tools.len()
                        );
                        server.tools = tools;
                        self.servers.insert(cfg.name.clone(), server);
                    }
                    Err(e) => {
                        tracing::warn!("MCP server '{}' initialization failed: {}", cfg.name, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("MCP server '{}' spawn failed: {}", cfg.name, e);
                }
            }
        }
        Ok(())
    }

    /// Collect all tools from all servers for the agent's tool list.
    pub fn all_tools(&self) -> Vec<ToolDef> {
        self.servers
            .values()
            .flat_map(|s| s.tools.iter().map(|t| t.to_tool_def(&s.name)))
            .collect()
    }

    /// Route a tool call to the correct server.
    pub async fn call(&mut self, server: &str, tool: &str, args: Value) -> Result<Value> {
        let srv = self
            .servers
            .get_mut(server)
            .ok_or_else(|| anyhow!("MCP server '{}' not found", server))?;
        srv.call_tool(tool, args).await
    }

    /// Graceful shutdown: send shutdown notification, wait, kill.
    pub async fn shutdown_all(&mut self) {
        for (name, mut server) in self.servers.drain() {
            if let Err(e) = server.shutdown().await {
                tracing::warn!("MCP server '{}' shutdown error: {}", name, e);
            }
        }
    }
}

impl McpToolServer {
    /// Spawn an MCP server subprocess.
    async fn spawn(cfg: &McpServerConfig) -> Result<Self> {
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        // Pass configured env vars
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }

        let mut process = cmd.spawn()?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdin"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;

        Ok(Self {
            name: cfg.name.clone(),
            tools: Vec::new(),
            process,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Initialize the MCP server and discover available tools.
    async fn initialize(&mut self) -> Result<Vec<McpTool>> {
        // Send initialize request
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "openkoi",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });
        self.send_request(&init_req).await?;
        let _init_resp = self.read_response().await?;

        // Send initialized notification
        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send_request(&initialized).await?;

        // List tools
        let list_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        self.send_request(&list_req).await?;
        let list_resp = self.read_response().await?;

        let tools = list_resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(McpTool {
                            name: v.get("name")?.as_str()?.to_string(),
                            description: v
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string(),
                            input_schema: v.get("inputSchema").cloned().unwrap_or(json!({})),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, params: Value) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": name, "arguments": params }
        });
        self.send_request(&request).await?;
        let response = self.read_response().await?;
        Ok(response.get("result").cloned().unwrap_or(json!(null)))
    }

    /// Graceful shutdown.
    async fn shutdown(&mut self) -> Result<()> {
        // Try to kill the process
        self.process.kill().await.ok();
        Ok(())
    }

    async fn send_request(&mut self, request: &Value) -> Result<()> {
        let line = serde_json::to_string(request)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_response(&mut self) -> Result<Value> {
        let mut line = String::new();
        self.stdout.read_line(&mut line).await?;
        let response: Value = serde_json::from_str(&line)?;
        Ok(response)
    }
}

/// Load MCP servers from .mcp.json (Claude Code / VS Code compatible).
pub fn discover_mcp_json(project_root: &Path) -> Vec<McpServerConfig> {
    let mcp_json = project_root.join(".mcp.json");
    if !mcp_json.exists() {
        return vec![];
    }

    let content = match std::fs::read_to_string(&mcp_json) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let parsed: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    parsed
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .map(|servers| {
            servers
                .iter()
                .filter_map(|(name, cfg)| {
                    let command = cfg.get("command")?.as_str()?.to_string();
                    let args = cfg
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let env = cfg
                        .get("env")
                        .and_then(|e| e.as_object())
                        .map(|obj| {
                            obj.iter()
                                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();

                    Some(McpServerConfig {
                        name: name.clone(),
                        command,
                        args,
                        env,
                        transport: "stdio".into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
