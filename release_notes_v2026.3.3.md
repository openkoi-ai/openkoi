## Executive Function as a Service

v2026.3.3 introduces a cognitive architecture for OpenKoi. Instead of jumping straight to code, OpenKoi now **thinks before it acts** — running every task through a Sovereign-Parliament deliberation pipeline that makes its reasoning inspectable, auditable, and improvable.

This is the largest feature release since OpenKoi's launch: 29 files changed, ~4,800 lines of new code across 4 new modules.

### New: Cognitive CLI Commands

Six new commands give you direct access to OpenKoi's reasoning:

| Command | What it does |
|---------|-------------|
| `openkoi think` | Flagship EFaaS pipeline — Sovereign frames intent, Parliament deliberates, then execute + evaluate + learn |
| `openkoi soul` | Inspect and evolve the Sovereign identity (`show`, `evolve`, `diff`, `history`) |
| `openkoi mind` | Explore the Society of Mind — five agencies that vote on every decision (`parliament`, `agencies`, `dissent`, `calibrate`) |
| `openkoi world` | Query the world model — tool atlas, domain atlas, human atlas (`show`, `tools`, `domains`, `human`) |
| `openkoi reflect` | Trigger feedback loops — daily, weekly, growth reviews and honest audits (`today`, `week`, `growth`, `audit`) |
| `openkoi trust` | Manage earned autonomy — grant, revoke, and audit delegation levels (`show`, `grant`, `revoke`, `audit`) |

### New: Sovereign-Parliament Architecture

- **Sovereign Directive**: A persistent identity layer that frames what the agent cares about and why, grounding every task in values before strategy
- **Parliament of Mind**: Five specialized agencies (Architect, Investigator, Critic, Pragmatist, Guardian) deliberate and vote on approach — with dissent recorded and visible
- **World Model**: Three internal atlases (Tool, Domain, Human) that accumulate knowledge across sessions
- **Reflection Loops**: Structured self-assessment at daily, weekly, and growth timescales with maturity tracking
- **Trust & Delegation**: Graduated autonomy levels (shadow → supervised → autonomous → trusted) earned through demonstrated competence

### New: `think` Pipeline

`openkoi think "your task"` runs the full cognitive pipeline:

1. **SOVEREIGN** — Frames intent from the soul's values
2. **PARLIAMENT** — Agencies deliberate; Guardian can block unsafe plans
3. **EXECUTE** — Carries out the chosen approach
4. **EVALUATE** — Assesses the result
5. **LEARN** — Extracts lessons into the world model

Use `--simulate` to see the full deliberation without executing. Use `--verbose` to see individual agency votes and confidence scores.

### Improvements

- Refactored daemon into modular components (`handler`, `process`, `scheduler`, `status`)
- Transitioned Store to asynchronous message-passing architecture
- New database migration (`003_mind_world_trust`) for cognitive state persistence
- Updated README with EFaaS branding, architecture diagram, and cognitive command documentation

### Housekeeping

- Zero `cargo clippy` warnings
- Zero `cargo fmt` diff
- All existing tests pass (412 unit + 57 integration)

---

**Full Changelog**: https://github.com/openkoi-ai/openkoi/compare/v2026.2.25...v2026.3.3

### Install / Upgrade

```bash
# Install (or upgrade)
curl -fsSL https://openkoi.ai/install.sh | bash

# Or via cargo
cargo install openkoi

# Verify
openkoi --version
# openkoi 2026.3.3

# Try the new cognitive pipeline
openkoi think "refactor the auth module for clarity"
```
