# statusmaxxx — one status line for every coding agent.

bin := justfile_directory() / "target/debug/statusmaxxx"

default:
    @just --list --unsorted

# === Develop ===

# Debug build; `try` and `sandbox` run this binary.
build:
    cargo build

# Run the CLI from source, e.g. `just run status`.
run *args:
    cargo run -q -- {{args}}

# Open the configuration TUI from source.
config:
    cargo run -q -- config

# Replay the session JSON <host> last sent, to see its line and any segment errors.
render host:
    cargo run -q -- render --host {{host}} < ~/.cache/statusmaxxx/payloads/{{host}}.json

# Put the release binary on PATH (~/.cargo/bin); agent wrappers point at it.
install:
    cargo install --path . --locked

# === Quality ===

# Format the tree.
fmt:
    cargo fmt

# Formatting and clippy, warnings as errors.
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# Unit tests.
test:
    cargo test

# What CI runs.
check: lint test

# Scan history and uncommitted changes for secrets.
secrets:
    gitleaks git --no-banner --redact
    gitleaks git --pre-commit --no-banner --redact

# === Real agents, real configs untouched ===

# Run a real agent on the debug build through its own settings override (claude, droid).
[no-cd]
try host: build
    #!/usr/bin/env bash
    set -euo pipefail
    scratch="$(mktemp -d)"
    wrapper="$scratch/statusline"
    printf '#!/bin/sh\nexec %q render --host %s\n' "{{bin}}" "{{host}}" > "$wrapper"
    chmod +x "$wrapper"
    case "{{host}}" in
        claude) entry='"padding":0' ;;
        droid) entry='"maxRows":1' ;;
        *) echo "{{host}} has no settings override flag; use \`just sandbox {{host}}\`" >&2; exit 1 ;;
    esac
    printf '{"statusLine":{"type":"command","command":"%s",%s}}\n' "$wrapper" "$entry" > "$scratch/settings.json"
    exec {{host}} --settings "$scratch/settings.json"

# Install into a throwaway HOME where every agent looks installed, then print the result.
sandbox +hosts: build
    #!/usr/bin/env bash
    set -euo pipefail
    home="$(mktemp -d)"
    for dir in .claude .cursor .qwen .factory .copilot .config/amp .pi/agent .config/opencode .codex .gemini; do
        mkdir -p "$home/$dir"
    done
    export HOME="$home" XDG_CONFIG_HOME= XDG_CACHE_HOME= CLAUDE_CONFIG_DIR= COPILOT_HOME= CODEX_HOME=
    "{{bin}}" install {{hosts}}
    "{{bin}}" status
    echo "Sandbox HOME: <$home>"

# === Release ===

# Show what the dist workflow will build for the current version.
dist-plan:
    dist plan

# Bump the version, commit, tag, and push; the dist workflow builds and publishes.
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    git diff --quiet && git diff --cached --quiet || { echo "Commit your changes first" >&2; exit 1; }
    just check
    cargo set-version "{{version}}"
    cargo check -q
    git commit -m "chore: release v{{version}}" -- Cargo.toml Cargo.lock
    git tag "v{{version}}"
    git push
    git push origin "v{{version}}"
