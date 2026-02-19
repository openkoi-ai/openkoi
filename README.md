# OpenKoi

A self-iterating AI agent system. Single binary, local-first, model-agnostic.

OpenKoi follows a **Plan-Execute-Evaluate-Refine** cycle — iterating on its own output until results meet your quality standards. It ships as a single static binary with zero runtime dependencies.

## Quick Start

```bash
# Install via Cargo
cargo install openkoi

# Or use the shell installer
curl -fsSL https://openkoi.ai/install.sh | sh
```

```bash
openkoi "Refactor the auth module to use JWT tokens"
```

OpenKoi detects your API keys from the environment, picks the best available model, and starts working. No config file needed.

## Features

- **Self-iteration** — Plan, execute, evaluate, refine. The agent is its own reviewer.
- **6 providers** — Anthropic, OpenAI, Google, Ollama, AWS Bedrock, any OpenAI-compatible endpoint.
- **Role-based models** — Assign different models to executor, evaluator, planner, and embedder roles.
- **Persistent memory** — SQLite + vector search. Learnings persist across sessions.
- **Pattern mining** — Observes your usage, proposes new skills to automate recurring workflows.
- **Skill system** — OpenClaw-compatible `.SKILL.md` format. Write once, use with any provider.
- **3-tier plugins** — MCP (external tools), WASM (sandboxed), Rhai (scripting).
- **10 integrations** — Slack, Discord, MS Teams, GitHub, Jira, Linear, Notion, Google Docs, Telegram, Email.
- **TUI dashboard** — Real-time view of tasks, costs, learnings, plugins, and config.
- **Soul system** — Optional personality that evolves with your interaction patterns.

## CLI

```bash
openkoi "task"              # Run a task (default 3 iterations)
openkoi chat                # Interactive REPL
openkoi learn               # Review proposed skills
openkoi status              # Show costs, memory, active models
openkoi doctor              # Run diagnostics
openkoi connect copilot     # Login to GitHub Copilot
openkoi connect chatgpt     # Login to ChatGPT Plus/Pro
openkoi connect slack       # Connect an integration
openkoi disconnect copilot  # Remove stored credentials
openkoi export all          # Export data as JSON/YAML
openkoi update              # Self-update
```

## Providers

### Subscription-based (OAuth login, free with your existing plan)

Use `openkoi connect` to authenticate via device-code flow. No API key needed.

| Provider | Command | Flow |
|----------|---------|------|
| GitHub Copilot | `openkoi connect copilot` | Device code (GitHub login) |
| ChatGPT Plus/Pro | `openkoi connect chatgpt` | Device code (OpenAI login) |

Tokens are stored in `~/.openkoi/auth.json` and refreshed automatically.

### API key providers

Set an environment variable or paste a key when prompted during `openkoi init`.

| Provider | Environment Variable |
|----------|---------------------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI | `OPENAI_API_KEY` |
| Google | `GOOGLE_API_KEY` |
| AWS Bedrock | `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY` |
| Groq | `GROQ_API_KEY` |
| OpenRouter | `OPENROUTER_API_KEY` |
| Together | `TOGETHER_API_KEY` |
| DeepSeek | `DEEPSEEK_API_KEY` |
| xAI | `XAI_API_KEY` |
| Qwen | `QWEN_API_KEY` |

API keys are saved to `~/.openkoi/credentials/<provider>.key` (owner-only permissions).

### Local

| Provider | Setup |
|----------|-------|
| Ollama | Auto-detected at `localhost:11434` |
| Custom (OpenAI-compatible) | `openkoi connect` picker or `config.toml` |

## Credential Discovery

OpenKoi auto-discovers credentials in this order:

1. **Environment variables** — `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`, etc.
2. **OAuth store** — GitHub Copilot, ChatGPT tokens from `openkoi connect`
3. **External CLIs** — Claude CLI (`~/.claude/.credentials.json`), Qwen CLI
4. **macOS Keychain** — Claude Code credentials (macOS only)
5. **Saved credentials** — `~/.openkoi/credentials/*.key`
6. **Local probes** — Ollama at `localhost:11434`

### Connect and Disconnect

```bash
# Login to a subscription provider
openkoi connect copilot     # GitHub Copilot
openkoi connect chatgpt     # ChatGPT Plus/Pro

# Remove stored credentials
openkoi disconnect copilot
openkoi disconnect chatgpt
openkoi disconnect anthropic   # Remove saved API key
openkoi disconnect all         # Remove all OAuth tokens

# Show connection status
openkoi connect status
```

## Architecture

```
              ┌─────────────────┐
              │   Orchestrator   │
              └────────┬────────┘
       ┌───────┬───────┼───────┬──────────┐
       ▼       ▼       ▼       ▼          ▼
   Executor Evaluator Learner Pattern   Integrations
       │                     Miner
       ▼
     Tools (MCP / WASM / Rhai)
```

Supported platforms: **Linux** (x86_64, ARM64) and **macOS** (Intel, Apple Silicon). Built with Rust + Tokio. < 10ms startup, ~5MB idle memory, ~20MB binary.

## Documentation

Full documentation at [openkoi.ai](https://openkoi.ai).

## License

[MIT](LICENSE)
