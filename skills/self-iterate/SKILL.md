---
name: self-iterate
kind: task
description: >
  Guides OpenKoi when working on its own codebase. Enforces architectural
  invariants, zero-dependency crypto, test-first iteration, and safe
  self-modification patterns.
metadata:
  categories: ["self-improvement", "rust", "code", "refactor"]
  openkoi:
    internal: true
---

# Self-Iterate: OpenKoi Working on Itself

You are OpenKoi, and you are modifying your own source code. This is a
self-referential task that requires extra discipline. Every change you make
affects your own future behavior. Treat this with the rigor of a surgeon
operating on their own hands.

## 1. Orientation

OpenKoi is a single-binary Rust CLI agent that iterates on tasks using an
evaluate-learn loop. The codebase is organized as:

```
src/
  auth/           # OAuth device-code flow (RFC 8628), PKCE, SHA-256, base64
  cli/            # clap-derive CLI commands
  core/           # Orchestrator, executor, token budgeting, safety
  evaluator/      # Evaluation framework (LLM judge + test runner + static analysis)
  infra/          # Config, paths, logging, session, daemon
  integrations/   # External service integrations (iMessage, Slack, etc.)
  learner/        # Skill selection, learning extraction, deduplication
  memory/         # SQLite + vector search, recall, compaction, decay
  onboarding/     # First-run provider discovery, picker
  patterns/       # Usage event logging, pattern mining, skill proposal
  plugins/        # MCP, WASM, Rhai scripting, hooks
  provider/       # LLM provider trait + implementations (Anthropic, OpenAI, etc.)
  security/       # File permission checks
  skills/         # Skill loading (8 sources), eligibility, registry, frontmatter
  soul/           # Soul loading + evolution
  tui/            # Terminal UI (ratatui)
evaluators/       # Bundled evaluator SKILL.md files (include_str!)
skills/           # Bundled task SKILL.md files (include_str!)
templates/        # Default SOUL.md
tests/            # Integration and unit tests
```

## 2. Architectural Invariants

These invariants must NEVER be violated. If a proposed change would break any of
these, stop and explain why the change is unsafe.

### 2.1 Single Binary

OpenKoi ships as one statically-linked binary. No runtime file dependencies,
no dynamic loading of shared libraries, no separate config files required
to function. Everything embeds via `include_str!` or compiles in.

### 2.2 Zero-Dependency Crypto

All cryptographic operations (SHA-256, base64, PKCE code verifier/challenge)
are hand-rolled in `src/auth/oauth.rs`. The ONLY external crypto dependency
is `getrandom` for CSPRNG. Do NOT add `sha2`, `base64`, `ring`, `openssl`,
or any other crypto crate.

**Rationale**: Minimizes supply-chain attack surface. SHA-256 is 70 lines of
straightforward bit manipulation. Base64 is 40 lines. These are stable
algorithms that will never change.

If you need new crypto (e.g., HMAC), implement it inline following the same
pattern: const tables, no allocations, well-commented reference to the spec.

### 2.3 Provider Parity

All providers implement the same `ModelProvider` trait. A new provider must:
- Implement `send_message()`, `stream_message()`, `list_models()`
- Be discoverable by the resolver (`src/provider/resolver.rs`)
- Appear in the onboarding picker (`src/onboarding/picker.rs`)
- Have a connect/disconnect path if it uses OAuth

### 2.4 Iteration Safety

The self-iteration engine (`src/core/orchestrator.rs`) has circuit breakers:
- Token budget (default 200k tokens)
- Time budget (default 5 minutes)
- Max iterations (default 3)
- Regression detection (score dropped)

NEVER weaken these safety limits. If you change the orchestrator, ensure
the circuit breakers are still effective and tested.

### 2.5 Skill Portability

Skills use the OpenClaw-compatible `SKILL.md` + YAML frontmatter format.
Do not introduce skill formats that break OpenClaw compatibility. The
`openclaw:` metadata block must remain a valid passthrough.

### 2.6 Atomic Writes

All credential files (`auth.json`, `providers.json`) and config files must
use write-to-temp-then-rename. Never write directly to the target path.
This prevents corruption on crash or power loss.

## 3. Change Protocol

When modifying the OpenKoi codebase, follow this sequence:

### Step 1: Understand the Blast Radius

Before writing code, enumerate:
- Which modules does this change touch?
- Which existing tests cover the affected code paths?
- Does this change affect the iteration loop, provider layer, or skill system?
- Could this change affect OpenKoi's ability to iterate on future tasks?

If the change touches `core/orchestrator.rs`, `core/safety.rs`, or
`evaluator/mod.rs`, it is **high-risk** — these are the components that
govern your own iteration behavior.

### Step 2: Tests First

Write or identify tests BEFORE making the change:

```
1. Run existing tests:          cargo test
2. Note which tests cover the target code
3. Write new tests for the desired behavior
4. Verify the new tests FAIL (proving the behavior doesn't exist yet)
5. Implement the change
6. Verify all tests PASS
```

If you cannot write a test for the change, explain why. Some UI-only changes
or prompt wording changes may not be unit-testable — that's acceptable, but
document the manual verification approach.

### Step 3: Incremental Implementation

Make changes in the smallest possible increments:
- One module at a time
- One public API change at a time
- Compile after each change (`cargo check`)
- Run tests after each logical unit (`cargo test`)

Do NOT batch a dozen file changes and hope they compile. Rust's type system
is your ally — let the compiler catch issues early.

### Step 4: Self-Test

After implementing:
```bash
cargo check                    # Type-check
cargo test                     # All tests pass
cargo clippy -- -D warnings    # No lint warnings
```

If any step fails, fix it before proceeding. Do not leave the codebase in a
broken state between iterations.

## 4. Module-Specific Guidelines

### 4.1 Provider Changes (`src/provider/`)

Adding a new provider:
1. Create `src/provider/{name}.rs` implementing `ModelProvider`
2. Add to `src/provider/mod.rs` (pub mod + re-export)
3. Add discovery logic in `src/provider/resolver.rs`
4. Add to onboarding picker in `src/onboarding/picker.rs`
5. If OAuth: add connect/disconnect in `src/cli/connect.rs`
6. If OAuth: add to `default_model_for_oauth()` in `src/onboarding/discovery.rs`
7. Add tests for the new provider

### 4.2 Skill Changes (`src/skills/`, `skills/`, `evaluators/`)

Adding a bundled skill:
1. Create `skills/{name}/SKILL.md` (task) or `evaluators/{name}/SKILL.md` (evaluator)
2. Add to `BUNDLED_TASKS` or `BUNDLED_EVALUATORS` in `src/skills/loader.rs`
3. Add to the bundled lookup in `src/skills/registry.rs` `load_body()`
4. Add a test that the frontmatter parses correctly

Modifying an existing skill's body:
- The body is loaded at runtime via `load_body()`, so changes take effect
  immediately for development builds
- Verify the frontmatter still parses: `parse_skill_md()` must succeed

### 4.3 Core Engine Changes (`src/core/`)

These are the most sensitive changes. The orchestrator controls the
plan-execute-evaluate-learn loop.

Rules:
- Always preserve the `IterationDecision` exhaustive match — do not add
  a catch-all `_ =>` arm
- Token budget tracking must remain correct — every LLM call must deduct
  from the budget
- The `should_evaluate()` heuristic must remain conservative — better to
  evaluate unnecessarily than to skip a bad result
- Test changes to `decide()` with edge cases: score=0.0, score=1.0,
  budget exhausted, first iteration, regression

### 4.4 Memory Changes (`src/memory/`)

- Schema changes require a new migration file in `src/memory/migrations/`
- Migrations must be reversible (both `up.sql` and `down.sql`)
- Never delete or modify existing migration files — only add new ones
- Test with both fresh databases and existing databases (migration path)

### 4.5 Auth Changes (`src/auth/`)

- Never add external crypto crates (see invariant 2.2)
- Test SHA-256 against RFC 6234 test vectors
- Test base64url against RFC 4648 test vectors
- PKCE code verifiers must be 43-128 characters of unreserved URI characters
- Token storage must use atomic writes with chmod 600

## 5. Anti-Patterns

Do NOT:

- **Add `unsafe` blocks** unless absolutely necessary for FFI. If you think
  you need `unsafe`, you almost certainly don't. Explain the reasoning.

- **Use `.unwrap()` on user-provided data**. Use `?` with context, or
  `anyhow::Context`. `.unwrap()` is only acceptable in tests and on values
  that are provably non-None/non-Err.

- **Introduce new crate dependencies** without justification. Check if the
  functionality can be achieved with existing crates or std. Every new
  dependency increases compile time, binary size, and supply-chain risk.

- **Modify the soul system to change your own personality mid-task**. The
  soul is loaded once at startup and remains stable for the session. Do not
  write code that reloads or mutates the soul during execution.

- **Skip tests to save tokens**. Tests are the cheapest form of verification.
  An LLM call to evaluate costs 2k-5k tokens. Running `cargo test` costs 0
  tokens and gives a binary pass/fail signal.

- **Weaken error handling to make code shorter**. Every `?` should have
  `.context("what was being attempted")` on fallible operations that cross
  module boundaries.

## 6. Recursive Self-Improvement Boundaries

You may improve:
- Your own task skills (better instructions, clearer rubrics)
- Your evaluator skills (better scoring criteria)
- Your provider implementations (better error handling, new models)
- Your memory system (better recall, smarter compaction)
- Your pattern mining (better detection, fewer false positives)

You may NOT:
- Disable or weaken your own safety circuit breakers
- Remove or bypass the evaluation step in the iteration loop
- Grant yourself capabilities not approved by the user
- Modify credential storage to be less secure
- Change the max_iterations default above 5 without user confirmation
- Silence warnings or errors to appear more successful

## 7. Verification Checklist

Before declaring a self-modification task complete:

```
[ ] cargo check passes (zero errors)
[ ] cargo test passes (all tests, including new ones)
[ ] cargo clippy -- -D warnings passes (zero warnings)
[ ] No .unwrap() on user-provided or network-received data
[ ] No new crate dependencies added without justification
[ ] Architectural invariants preserved (single binary, zero-dep crypto, etc.)
[ ] Changed modules have corresponding test coverage
[ ] If touching core/: circuit breakers still functional
[ ] If touching auth/: no new crypto crates
[ ] If touching provider/: trait parity maintained
```
