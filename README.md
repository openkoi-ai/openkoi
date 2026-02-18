# OpenKoi

A self-iterating AI agent system. Single binary, local-first, model-agnostic.

OpenKoi follows a **Plan-Execute-Evaluate-Refine** cycle — iterating on its own output until results meet your quality standards. It ships as a single static binary with zero runtime dependencies.

## Quick Start

```bash
# Install via Cargo
cargo install openkoi

# Or use the shell installer
curl -fsSL https://openkoi.dev/install.sh | sh
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
openkoi connect slack       # Connect an integration
openkoi export all          # Export data as JSON/YAML
openkoi update              # Self-update
```

## Credential Discovery

OpenKoi auto-discovers credentials in this order:

1. **Environment variables** — `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY`, `AWS_ACCESS_KEY_ID`
2. **External CLIs** — Claude CLI, OpenAI Codex credentials
3. **Local probes** — Ollama at `localhost:11434`

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

Built with Rust + Tokio. < 10ms startup, ~5MB idle memory, ~20MB binary.

## Documentation

Full documentation at [openkoi.ai](https://openkoi.ai).

## License

[MIT](LICENSE)
