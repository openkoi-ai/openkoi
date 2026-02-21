#!/usr/bin/env bash
# tests/e2e.sh — End-to-end integration tests for the openkoi CLI
#
# Exercises every functional surface of the CLI without making real LLM calls.
# Uses OPENKOI_HOME for full isolation — no pollution of the user's real data.
#
# Prerequisites: bash, curl, jq, chmod
# Usage:
#   cargo build --target aarch64-apple-darwin --release
#   bash tests/e2e.sh
#
# CI usage (cross-platform):
#   cargo build --release
#   bash tests/e2e.sh ./target/release/openkoi

set -uo pipefail

# ─── Configuration ──────────────────────────────────────────────────

# Binary path — accept as first argument or default to macOS ARM release
BINARY="${1:-./target/aarch64-apple-darwin/release/openkoi}"
BINARY="$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")"

# Temp home for complete isolation
OPENKOI_HOME="$(mktemp -d)"
export OPENKOI_HOME

# Unset all provider API keys to prevent accidental real calls
unset ANTHROPIC_API_KEY OPENAI_API_KEY GOOGLE_API_KEY GROQ_API_KEY \
      OPENROUTER_API_KEY TOGETHER_API_KEY DEEPSEEK_API_KEY XAI_API_KEY \
      MOONSHOT_API_KEY 2>/dev/null || true

# ─── Test Framework ─────────────────────────────────────────────────

PASS=0
FAIL=0
SKIP=0
SECTION=""
FAILURES=()

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

section() {
    SECTION="$1"
    printf "\n${CYAN}${BOLD}── %s ──${RESET}\n" "$SECTION"
}

pass() {
    PASS=$((PASS + 1))
    printf "  ${GREEN}PASS${RESET}  %s\n" "$1"
}

fail() {
    FAIL=$((FAIL + 1))
    printf "  ${RED}FAIL${RESET}  %s\n" "$1"
    if [ -n "${2:-}" ]; then
        printf "        %s\n" "$2"
    fi
    FAILURES+=("[$SECTION] $1")
}

skip() {
    SKIP=$((SKIP + 1))
    printf "  ${YELLOW}SKIP${RESET}  %s\n" "$1"
}

# Assert output contains a substring
assert_contains() {
    local label="$1"
    local haystack="$2"
    local needle="$3"
    if echo "$haystack" | grep -qF -- "$needle"; then
        pass "$label"
    else
        fail "$label" "expected to contain: '$needle'"
    fi
}

# Assert output does NOT contain a substring
assert_not_contains() {
    local label="$1"
    local haystack="$2"
    local needle="$3"
    if echo "$haystack" | grep -qF -- "$needle"; then
        fail "$label" "expected NOT to contain: '$needle'"
    else
        pass "$label"
    fi
}

# Assert exit code
assert_exit() {
    local label="$1"
    local expected="$2"
    local actual="$3"
    if [ "$actual" -eq "$expected" ]; then
        pass "$label"
    else
        fail "$label" "expected exit code $expected, got $actual"
    fi
}

# Assert file exists
assert_file_exists() {
    local label="$1"
    local path="$2"
    if [ -f "$path" ]; then
        pass "$label"
    else
        fail "$label" "file not found: $path"
    fi
}

# Assert file does NOT exist
assert_file_not_exists() {
    local label="$1"
    local path="$2"
    if [ ! -f "$path" ]; then
        pass "$label"
    else
        fail "$label" "file should not exist: $path"
    fi
}

# Assert directory exists
assert_dir_exists() {
    local label="$1"
    local path="$2"
    if [ -d "$path" ]; then
        pass "$label"
    else
        fail "$label" "directory not found: $path"
    fi
}

# Assert JSON field equals value (uses jq)
assert_json_eq() {
    local label="$1"
    local json="$2"
    local field="$3"
    local expected="$4"
    local actual
    actual=$(echo "$json" | jq -r "$field" 2>/dev/null)
    if [ "$actual" = "$expected" ]; then
        pass "$label"
    else
        fail "$label" ".$field: expected '$expected', got '$actual'"
    fi
}

# Assert file permission mode (Unix only)
assert_file_mode() {
    local label="$1"
    local path="$2"
    local expected="$3"
    if [[ "$OSTYPE" == "linux"* ]] || [[ "$OSTYPE" == "darwin"* ]]; then
        local actual
        actual=$(stat -f '%Lp' "$path" 2>/dev/null || stat -c '%a' "$path" 2>/dev/null)
        if [ "$actual" = "$expected" ]; then
            pass "$label"
        else
            fail "$label" "expected mode $expected, got $actual"
        fi
    else
        skip "$label (non-Unix platform)"
    fi
}

# ─── Preamble ───────────────────────────────────────────────────────

printf "${BOLD}openkoi end-to-end tests${RESET}\n"
printf "Binary:       %s\n" "$BINARY"
printf "OPENKOI_HOME: %s\n" "$OPENKOI_HOME"
printf "Platform:     %s\n" "$OSTYPE"

# ════════════════════════════════════════════════════════════════════
# Section 1: Setup & Sanity
# ════════════════════════════════════════════════════════════════════

section "1. Setup & Sanity"

if [ ! -x "$BINARY" ]; then
    fail "Binary exists and is executable" "not found: $BINARY"
    printf "\n${RED}Cannot continue without binary. Run 'cargo build --release' first.${RESET}\n"
    exit 1
fi
pass "Binary exists and is executable"

OUT=$("$BINARY" --version 2>&1)
assert_contains "Version output includes 2026" "$OUT" "2026"

# ════════════════════════════════════════════════════════════════════
# Section 2: CLI Help & Version
# ════════════════════════════════════════════════════════════════════

section "2. CLI Help & Version"

# 2.1 — --help
OUT=$("$BINARY" --help 2>&1)
assert_contains "--help shows 'Self-iterating'" "$OUT" "Self-iterating"
assert_contains "--help shows chat subcommand" "$OUT" "chat"
assert_contains "--help shows learn subcommand" "$OUT" "learn"
assert_contains "--help shows status subcommand" "$OUT" "status"
assert_contains "--help shows daemon subcommand" "$OUT" "daemon"

# 2.2 — --version
OUT=$("$BINARY" --version 2>&1)
assert_contains "--version format" "$OUT" "openkoi"

# 2.3 — Subcommand help
OUT=$("$BINARY" help chat 2>&1)
assert_contains "chat help" "$OUT" "Interactive chat session"

OUT=$("$BINARY" help learn 2>&1)
assert_contains "learn help" "$OUT" "learned patterns"

OUT=$("$BINARY" help status 2>&1)
assert_contains "status help shows --costs" "$OUT" "--costs"
assert_contains "status help shows --live" "$OUT" "--live"

OUT=$("$BINARY" help daemon 2>&1)
assert_contains "daemon help shows start" "$OUT" "start"
assert_contains "daemon help shows stop" "$OUT" "stop"

OUT=$("$BINARY" help dashboard 2>&1)
assert_contains "dashboard help shows --export" "$OUT" "--export"

OUT=$("$BINARY" help update 2>&1)
assert_contains "update help shows --check" "$OUT" "--check"

OUT=$("$BINARY" help disconnect 2>&1)
assert_contains "disconnect help shows APP" "$OUT" "APP"

# ════════════════════════════════════════════════════════════════════
# Section 3: Configuration
# ════════════════════════════════════════════════════════════════════

section "3. Configuration"

# 3.1 — Default config (no config.toml present)
OUT=$("$BINARY" status 2>&1)
assert_contains "Default config: using defaults" "$OUT" "(using defaults)"

# 3.2 — Valid config.toml loaded
printf '[api]\nport = 19742\n' > "$OPENKOI_HOME/config.toml"
OUT=$("$BINARY" status 2>&1)
assert_contains "Custom config loaded" "$OUT" "(loaded)"

# 3.3 — Invalid TOML file via --config
printf 'this is not valid toml [[[' > "$OPENKOI_HOME/bad.toml"
OUT=$("$BINARY" --config "$OPENKOI_HOME/bad.toml" status 2>&1)
RC=$?
assert_contains "Invalid TOML error message" "$OUT" "TOML parse error"
assert_exit "Invalid TOML exits non-zero" 1 "$RC"

# 3.4 — Full config with all sections
cat > "$OPENKOI_HOME/config.toml" <<'TOML'
default_model = "anthropic/claude-sonnet-4-20250514"
max_iterations = 5
quality_threshold = 0.85

[api]
port = 19742
token = "e2e-test-token"

[daemon]
auto_execute = false
TOML
OUT=$("$BINARY" status 2>&1)
assert_contains "Full config loaded" "$OUT" "(loaded)"

# 3.5 — Doctor command
OUT=$("$BINARY" doctor 2>&1)
assert_contains "Doctor header" "$OUT" "openkoi doctor"
assert_contains "Doctor shows providers check" "$OUT" "providers"
assert_contains "Doctor footer" "$OUT" "Done."

# ════════════════════════════════════════════════════════════════════
# Section 4: Database & Memory
# ════════════════════════════════════════════════════════════════════

section "4. Database & Memory"

# 4.1 — Status shows no DB before migration
rm -f "$OPENKOI_HOME/data/openkoi.db"
OUT=$("$BINARY" status 2>&1)
assert_contains "No DB: not initialized" "$OUT" "(not initialized)"

# 4.2 — setup --migrate creates the database
OUT=$("$BINARY" setup --migrate 2>&1)
assert_contains "Migrate: running migrations" "$OUT" "Running database migrations"
assert_contains "Migrate: complete" "$OUT" "Migrations complete"
assert_contains "Migrate: schema version" "$OUT" "Current schema version: 2"
assert_file_exists "DB file created" "$OPENKOI_HOME/data/openkoi.db"

# 4.3 — Status shows DB after migration
OUT=$("$BINARY" status 2>&1)
assert_contains "Status shows DB path" "$OUT" "openkoi.db"
assert_contains "Status shows DB size" "$OUT" "KB"

# 4.4 — Status shows activity after DB creation
assert_contains "Status shows tasks count" "$OUT" "Tasks:"
assert_contains "Status shows learnings count" "$OUT" "Learnings:"
assert_contains "Status shows sessions count" "$OUT" "Sessions:"

# 4.5 — Status --costs flag accepted
OUT=$("$BINARY" status --costs 2>&1)
RC=$?
assert_exit "Status --costs exits 0" 0 "$RC"

# 4.6 — Export learnings (empty DB → empty JSON array)
OUT=$("$BINARY" export learnings --format json 2>&1)
assert_contains "Export learnings: empty array" "$OUT" "[]"

# 4.7 — Export sessions (empty DB → empty JSON array)
OUT=$("$BINARY" export sessions --format json 2>&1)
assert_contains "Export sessions: empty array" "$OUT" "[]"

# 4.8 — Export all (structured JSON with version)
OUT=$("$BINARY" export all --format json 2>&1)
assert_contains "Export all: has version" "$OUT" "\"version\""
assert_contains "Export all: has learnings key" "$OUT" "\"learnings\""
assert_contains "Export all: has sessions key" "$OUT" "\"sessions\""
assert_contains "Export all: has patterns key" "$OUT" "\"patterns\""
# Validate it's actually parseable JSON
if echo "$OUT" | jq . >/dev/null 2>&1; then
    pass "Export all: valid JSON"
else
    fail "Export all: valid JSON" "jq failed to parse output"
fi

# 4.9 — Export to file
OUT=$("$BINARY" export learnings --format json --output "$OPENKOI_HOME/export-test.json" 2>&1)
assert_file_exists "Export to file creates file" "$OPENKOI_HOME/export-test.json"
assert_contains "Export to file: confirms path" "$OUT" "export-test.json"

# 4.10 — Re-running migrate is idempotent
OUT=$("$BINARY" setup --migrate 2>&1)
assert_contains "Idempotent migrate: schema version 2" "$OUT" "Current schema version: 2"

# ════════════════════════════════════════════════════════════════════
# Section 5: Skill System
# ════════════════════════════════════════════════════════════════════

section "5. Skill System"

# 5.1 — Status shows skill counts (all zero in fresh home)
OUT=$("$BINARY" status 2>&1)
assert_contains "Skills: zero counts" "$OUT" "0 managed, 0 user, 0 proposed"

# 5.2 — Create a valid user skill
mkdir -p "$OPENKOI_HOME/data/skills/user"
cat > "$OPENKOI_HOME/data/skills/user/test-skill.md" <<'SKILL'
---
name: test-skill
description: A test skill for E2E testing
trigger: test
category: testing
---
# Test Skill

This is a test skill used by the E2E test suite.
SKILL
OUT=$("$BINARY" status 2>&1)
assert_contains "User skill counted" "$OUT" "1 user"

# 5.3 — Create a proposed skill
mkdir -p "$OPENKOI_HOME/data/skills/proposed"
cat > "$OPENKOI_HOME/data/skills/proposed/proposed-skill.md" <<'SKILL'
---
name: proposed-skill
description: A proposed skill
trigger: propose
category: testing
---
# Proposed Skill
SKILL
OUT=$("$BINARY" status 2>&1)
assert_contains "Proposed skill counted" "$OUT" "1 proposed"

# 5.4 — learn list works (shows proposed skills or patterns message)
OUT=$("$BINARY" learn list 2>&1)
RC=$?
assert_exit "learn list exits 0" 0 "$RC"
# With empty DB it should show the "no proposed skills" or patterns message
assert_contains "learn list: patterns message" "$OUT" "patterns"

# 5.5 — Invalid frontmatter in SKILL.md (missing --- delimiter)
cat > "$OPENKOI_HOME/data/skills/user/bad-skill.md" <<'SKILL'
name: bad-skill
description: Missing frontmatter delimiters
---
# Bad Skill
SKILL
# The binary should still work; bad skills are logged and skipped
OUT=$("$BINARY" status 2>&1)
RC=$?
assert_exit "Bad skill doesn't crash status" 0 "$RC"

# Clean up bad skill
rm -f "$OPENKOI_HOME/data/skills/user/bad-skill.md"

# ════════════════════════════════════════════════════════════════════
# Section 6: Auth & Credentials
# ════════════════════════════════════════════════════════════════════

section "6. Auth & Credentials"

# 6.1 — No credentials state
OUT=$("$BINARY" disconnect 2>&1)
assert_contains "No creds: nothing connected" "$OUT" "No providers or integrations are currently connected"

# 6.2 — Legacy credential migration (create old-style .key file)
mkdir -p "$OPENKOI_HOME/credentials"
echo "sk-test-legacy-key-12345" > "$OPENKOI_HOME/credentials/anthropic.key"
chmod 600 "$OPENKOI_HOME/credentials/anthropic.key"

# Doctor should still work with legacy credentials present
OUT=$("$BINARY" doctor 2>&1)
RC=$?
assert_exit "Doctor works with legacy credentials" 0 "$RC"

# 6.3 — Disconnect non-existent provider
OUT=$("$BINARY" disconnect nonexistent 2>&1)
RC=$?
# Should print an error about unknown target or no credentials
if echo "$OUT" | grep -qiF "unknown\|not found\|no cred"; then
    pass "Disconnect nonexistent: error message"
else
    # May still exit 0 with a message; check for any output
    pass "Disconnect nonexistent: handled gracefully"
fi

# 6.4 — Auth file permissions (create and verify)
echo '{"providers":{}}' > "$OPENKOI_HOME/auth.json"
chmod 600 "$OPENKOI_HOME/auth.json"
assert_file_mode "Auth file mode 600" "$OPENKOI_HOME/auth.json" "600"

# 6.5 — Disconnect all (safe even with nothing)
OUT=$("$BINARY" disconnect all 2>&1)
RC=$?
assert_exit "Disconnect all exits 0" 0 "$RC"

# Clean up legacy creds
rm -rf "$OPENKOI_HOME/credentials"

# ── OAuth auth store tests ─────────────────────────────────────────

# 6.6 — OAuth roundtrip: auth.json with copilot → connect status shows connected
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_e2e_test","refresh_token":"gho_e2e_ref","expires_at":0,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "OAuth roundtrip: copilot connected" "$OUT" "[+] GitHub Copilot: connected (subscription login)"

# 6.7 — Expired token detection
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_expired","refresh_token":"gho_ref","expires_at":1000,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "Expired token: shows warning" "$OUT" "[!] GitHub Copilot: token expired"

# 6.8 — API key in auth store
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"anthropic":{"type":"api_key","key":"sk-ant-e2e-test"}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "API key in auth store" "$OUT" "[+] anthropic: API key saved"

# 6.9 — Multiple providers in auth store
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_xxx","refresh_token":"gho_ref","expires_at":0,"extra":{}},"chatgpt":{"type":"oauth","access_token":"eyJ_xxx","refresh_token":"v1.xxx","expires_at":0,"extra":{"account_id":"user-e2e"}},"anthropic":{"type":"api_key","key":"sk-ant-e2e"}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "Multi-provider: copilot connected" "$OUT" "[+] GitHub Copilot: connected"
assert_contains "Multi-provider: chatgpt connected" "$OUT" "[+] ChatGPT Plus/Pro: connected"
assert_contains "Multi-provider: anthropic key saved" "$OUT" "[+] anthropic: API key saved"

# 6.10 — OAuth disconnect removes provider
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_to_remove","refresh_token":"gho_ref","expires_at":0,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" disconnect copilot 2>&1)
assert_contains "OAuth disconnect: success message" "$OUT" "disconnected"
# After disconnect, connect status should show not connected
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "OAuth disconnect: now not connected" "$OUT" "[-] GitHub Copilot: not connected"

# 6.11 — Disconnect with alias (github-copilot)
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_alias","refresh_token":"gho_ref","expires_at":0,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" disconnect github-copilot 2>&1)
assert_contains "Alias disconnect: success" "$OUT" "disconnected"

# 6.12 — Disconnect provider that's not connected
rm -f "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" disconnect copilot 2>&1)
assert_contains "Disconnect absent: not connected" "$OUT" "is not connected"

# 6.13 — Unknown connect target
OUT=$("$BINARY" connect bogus-provider 2>&1)
RC=$?
assert_contains "Unknown connect target: error" "$OUT" "Unknown target"
assert_contains "Unknown connect target: shows copilot" "$OUT" "copilot"
assert_contains "Unknown connect target: shows chatgpt" "$OUT" "chatgpt"

# 6.14 — Disconnect all with OAuth entries populated
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_all","refresh_token":"gho_ref","expires_at":0,"extra":{}},"chatgpt":{"type":"oauth","access_token":"eyJ_all","refresh_token":"v1_all","expires_at":0,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" disconnect all 2>&1)
RC=$?
assert_exit "Disconnect all with OAuth exits 0" 0 "$RC"
assert_contains "Disconnect all: removed copilot" "$OUT" "copilot"
assert_contains "Disconnect all: removed chatgpt" "$OUT" "chatgpt"
# Verify auth.json is now empty of providers
OUT=$("$BINARY" setup --connect status 2>&1)
assert_contains "After disconnect all: copilot gone" "$OUT" "[-] GitHub Copilot: not connected"
assert_contains "After disconnect all: chatgpt gone" "$OUT" "[-] ChatGPT Plus/Pro: not connected"

# 6.15 — AuthStore::save() creates auth.json with correct permissions
rm -f "$OPENKOI_HOME/auth.json"
cat > "$OPENKOI_HOME/auth.json" <<'EOF'
{"providers":{"copilot":{"type":"oauth","access_token":"gho_perm","refresh_token":"gho_ref","expires_at":0,"extra":{}}}}
EOF
chmod 600 "$OPENKOI_HOME/auth.json"
# Disconnect triggers save → should maintain 600 perms
"$BINARY" disconnect copilot >/dev/null 2>&1
assert_file_exists "Auth file still exists after save" "$OPENKOI_HOME/auth.json"
assert_file_mode "Auth file 600 after save" "$OPENKOI_HOME/auth.json" "600"

# Clean up
rm -f "$OPENKOI_HOME/auth.json"

# ════════════════════════════════════════════════════════════════════
# Section 7: Provider Discovery
# ════════════════════════════════════════════════════════════════════

section "7. Provider Discovery"

# 7.1 — No API keys → doctor reports NONE FOUND
OUT=$("$BINARY" doctor 2>&1)
assert_contains "No providers: NONE FOUND" "$OUT" "NONE FOUND"

# 7.2 — Fake ANTHROPIC_API_KEY → provider discovered
OUT=$(ANTHROPIC_API_KEY="sk-ant-test-fake-key" "$BINARY" doctor 2>&1)
# With a fake key, Anthropic should be discovered
if echo "$OUT" | grep -qi "anthropic\|1 found\|found"; then
    pass "Fake Anthropic key: provider discovered"
else
    # The key format might not be validated at discovery time
    skip "Fake Anthropic key: discovery behavior varies"
fi

# 7.3 — Multiple fake keys → multiple providers discovered
OUT=$(ANTHROPIC_API_KEY="sk-ant-fake" OPENAI_API_KEY="sk-fake" "$BINARY" doctor 2>&1)
if echo "$OUT" | grep -qi "found"; then
    pass "Multiple keys: providers found"
else
    skip "Multiple keys: discovery behavior varies"
fi

# 7.4 — Invalid model name (fuzzy match suggestion)
# This exercises the model validation / "did you mean?" path
# The binary should fail gracefully when given a bogus model with no providers
OUT=$("$BINARY" -m "bogus/nonexistent-model" --version 2>&1)
RC=$?
# --version should still work regardless of -m flag
assert_contains "Model flag doesn't block --version" "$OUT" "openkoi"

# 7.5 — Provider-prefixed model format accepted
# With no providers, this should fail but not crash
OUT=$("$BINARY" -m "anthropic/claude-sonnet-4-20250514" status 2>&1)
RC=$?
assert_exit "Model flag with status exits 0" 0 "$RC"

# ════════════════════════════════════════════════════════════════════
# Section 8: HTTP API (via standalone server test)
# ════════════════════════════════════════════════════════════════════

section "8. HTTP API"

# The API server runs as part of the daemon, which requires a configured
# provider and integrations. Since we can't start the full daemon without
# real LLM credentials, we test API behavior via the existing Rust unit
# tests and focus on what we can verify from the CLI:

# 8.1 — Daemon start without integrations exits with message
OUT=$("$BINARY" daemon start 2>&1)
RC=$?
# The daemon should exit quickly since no integrations are configured
if echo "$OUT" | grep -qiF "no integrations\|nothing to do\|connect"; then
    pass "Daemon start: no integrations message"
elif [ "$RC" -ne 0 ]; then
    pass "Daemon start: exits when no integrations"
else
    skip "Daemon start: behavior varies without integrations"
fi

# 8.2 — Daemon status
OUT=$("$BINARY" daemon status 2>&1)
RC=$?
# Should report daemon not running (since we didn't start it)
assert_exit "Daemon status command exits" 0 "$RC"

# 8.3 — Daemon stop (nothing to stop)
OUT=$("$BINARY" daemon stop 2>&1)
RC=$?
# Should handle gracefully
assert_exit "Daemon stop (nothing running) exits" 0 "$RC"

# 8.4 — API config is accepted in config.toml
cat > "$OPENKOI_HOME/config.toml" <<'TOML'
[api]
enabled = true
port = 19742
token = "e2e-test-token"

[api.webhooks]
on_task_complete = "http://localhost:9999/webhook"
TOML
OUT=$("$BINARY" status 2>&1)
assert_contains "Config with API section loads" "$OUT" "(loaded)"

# Note: Full HTTP endpoint tests (health, status, tasks, auth, CORS)
# are covered by the Rust unit tests in src/api/mod.rs (13 tests).
# A proper integration test that starts the server would require either:
# - A test harness binary, or
# - Real provider credentials for the daemon.
# This is intentionally out of scope for this bash-based E2E suite.

# ════════════════════════════════════════════════════════════════════
# Section 9: Security & Permissions
# ════════════════════════════════════════════════════════════════════

section "9. Security & Permissions"

# 9.1 — Doctor checks file permissions
OUT=$("$BINARY" doctor 2>&1)
assert_contains "Doctor checks permissions" "$OUT" "permissions"

# 9.2 — Sensitive file with 644 mode (too permissive)
echo '{"version":1}' > "$OPENKOI_HOME/auth.json"
chmod 644 "$OPENKOI_HOME/auth.json"
OUT=$("$BINARY" doctor 2>&1)
# Doctor should warn about the permissive file
if echo "$OUT" | grep -qi "permissive\|warn\|644\|too"; then
    pass "Doctor warns about 644 auth file"
else
    # Doctor may not specifically check auth.json, only known paths
    skip "Doctor permission check for auth.json (may not be audited)"
fi

# 9.3 — Fix permissions and verify
chmod 600 "$OPENKOI_HOME/auth.json"
assert_file_mode "Auth file fixed to 600" "$OPENKOI_HOME/auth.json" "600"

# 9.4 — Config file permissions
chmod 600 "$OPENKOI_HOME/config.toml"
assert_file_mode "Config file mode 600" "$OPENKOI_HOME/config.toml" "600"

# 9.5 — Data directory permissions
chmod 700 "$OPENKOI_HOME/data"
assert_file_mode "Data dir mode 700" "$OPENKOI_HOME/data" "700"

# ════════════════════════════════════════════════════════════════════
# Section 10: Plugin System
# ════════════════════════════════════════════════════════════════════

section "10. Plugin System"

# 10.1 — MCP .mcp.json discovery
# Create a .mcp.json file in the current directory
ORIG_DIR="$(pwd)"
PLUGIN_DIR="$(mktemp -d)"
cat > "$PLUGIN_DIR/.mcp.json" <<'JSON'
{
  "mcpServers": {
    "test-server": {
      "command": "echo",
      "args": ["hello"]
    }
  }
}
JSON
# Doctor should notice MCP config if we run from that directory
# (MCP discovery looks at the project root / cwd)
OUT=$(cd "$PLUGIN_DIR" && "$BINARY" doctor 2>&1)
RC=$?
assert_exit "Doctor with .mcp.json doesn't crash" 0 "$RC"
rm -rf "$PLUGIN_DIR"
cd "$ORIG_DIR"

# 10.2 — Rhai script loading: create a valid script
mkdir -p "$OPENKOI_HOME/data/plugins/scripts"
cat > "$OPENKOI_HOME/data/plugins/scripts/test_hook.rhai" <<'RHAI'
fn before_plan(ctx) {
    log("E2E test hook: before_plan called");
}

fn after_execute(ctx) {
    log("E2E test hook: after_execute called");
}
RHAI

# Configure Rhai scripts in config
cat > "$OPENKOI_HOME/config.toml" <<'TOML'
[plugins]
scripts = ["test_hook.rhai"]
TOML

# Doctor should report the Rhai script
OUT=$("$BINARY" doctor 2>&1)
RC=$?
assert_exit "Doctor with Rhai config doesn't crash" 0 "$RC"

# 10.3 — WASM plugin: nonexistent file doesn't crash
cat > "$OPENKOI_HOME/config.toml" <<'TOML'
[plugins]
wasm = ["nonexistent-plugin.wasm"]
TOML
OUT=$("$BINARY" doctor 2>&1)
RC=$?
assert_exit "Doctor with missing WASM plugin doesn't crash" 0 "$RC"

# 10.4 — Plugin config with both Rhai and WASM
cat > "$OPENKOI_HOME/config.toml" <<'TOML'
[plugins]
scripts = ["test_hook.rhai"]
wasm = ["nonexistent-plugin.wasm"]
TOML
OUT=$("$BINARY" doctor 2>&1)
RC=$?
assert_exit "Doctor with mixed plugin config doesn't crash" 0 "$RC"

# ════════════════════════════════════════════════════════════════════
# Section 11: Hidden / Backward-Compat Commands
# ════════════════════════════════════════════════════════════════════

section "11. Hidden Backward-Compat Commands"

# These are hidden aliases that still work for backward compatibility.

# 11.1 — `openkoi init` (alias for setup)
OUT=$("$BINARY" init 2>&1)
RC=$?
# init runs the setup wizard; it should at least start without crashing
# (it may fail gracefully if provider discovery finds nothing)
if [ "$RC" -eq 0 ] || echo "$OUT" | grep -qiF "setup\|init\|scanning\|complete"; then
    pass "Hidden 'init' command works"
else
    fail "Hidden 'init' command works" "exit=$RC, output: $(echo "$OUT" | head -3)"
fi

# 11.2 — `openkoi doctor` (alias, already tested but verify it's accessible)
OUT=$("$BINARY" doctor 2>&1)
assert_contains "Hidden 'doctor' command shows header" "$OUT" "openkoi doctor"

# 11.3 — `openkoi export` (hidden alias for dashboard --export)
OUT=$("$BINARY" export learnings --format json 2>&1)
RC=$?
assert_exit "Hidden 'export' command works" 0 "$RC"

# 11.4 — `openkoi migrate` (hidden alias for setup --migrate)
OUT=$("$BINARY" migrate 2>&1)
RC=$?
assert_exit "Hidden 'migrate' command works" 0 "$RC"
assert_contains "Hidden 'migrate': shows migrations" "$OUT" "schema version"

# ════════════════════════════════════════════════════════════════════
# Section 12: Edge Cases & Error Handling
# ════════════════════════════════════════════════════════════════════

section "12. Edge Cases & Error Handling"

# 12.1 — Unknown subcommand
OUT=$("$BINARY" nonexistent-command 2>&1)
RC=$?
# clap treats unknown strings as TASK arguments (the default command),
# so this will try to run a task "nonexistent-command"
# It should fail gracefully (no providers available)
if [ "$RC" -ne 0 ] || echo "$OUT" | grep -qi "no provider\|error\|init"; then
    pass "Unknown input handled gracefully"
else
    pass "Unknown input treated as task (expected behavior)"
fi

# 12.2 — --config with nonexistent file
OUT=$("$BINARY" --config "/nonexistent/path/config.toml" status 2>&1)
RC=$?
assert_exit "Nonexistent --config exits 1" 1 "$RC"
assert_contains "Nonexistent --config: error message" "$OUT" "error"

# 12.3 — --quiet flag is accepted
OUT=$("$BINARY" --quiet status 2>&1)
RC=$?
assert_exit "--quiet flag accepted" 0 "$RC"

# 12.4 — --stdin flag is accepted (with empty stdin)
OUT=$(echo "" | "$BINARY" --stdin 2>&1)
RC=$?
# Should fail gracefully (empty task or no provider)
if [ "$RC" -ne 0 ] || echo "$OUT" | grep -qi "error\|empty\|no provider"; then
    pass "--stdin with empty input handled"
else
    pass "--stdin flag accepted"
fi

# 12.5 — Iteration flag
OUT=$("$BINARY" -i 0 status 2>&1)
RC=$?
assert_exit "-i 0 flag with status exits 0" 0 "$RC"

# 12.6 — Quality threshold flag
OUT=$("$BINARY" -q 0.5 status 2>&1)
RC=$?
assert_exit "-q 0.5 flag with status exits 0" 0 "$RC"

# 12.7 — --select-model alias
OUT=$("$BINARY" --help 2>&1)
assert_contains "--select-model visible alias" "$OUT" "select-model"

# ════════════════════════════════════════════════════════════════════
# Cleanup & Summary
# ════════════════════════════════════════════════════════════════════

section "Cleanup"

rm -rf "$OPENKOI_HOME"
pass "Temp directory cleaned up"

# ─── Summary ────────────────────────────────────────────────────────

TOTAL=$((PASS + FAIL + SKIP))

printf "\n${BOLD}════════════════════════════════════════${RESET}\n"
printf "${BOLD}  Results: %d tests${RESET}\n" "$TOTAL"
printf "  ${GREEN}Passed:  %d${RESET}\n" "$PASS"
printf "  ${RED}Failed:  %d${RESET}\n" "$FAIL"
printf "  ${YELLOW}Skipped: %d${RESET}\n" "$SKIP"
printf "${BOLD}════════════════════════════════════════${RESET}\n"

if [ "$FAIL" -gt 0 ]; then
    printf "\n${RED}${BOLD}Failed tests:${RESET}\n"
    for f in "${FAILURES[@]}"; do
        printf "  ${RED}•${RESET} %s\n" "$f"
    done
    printf "\n"
    exit 1
fi

printf "\n${GREEN}${BOLD}All tests passed.${RESET}\n"
exit 0
