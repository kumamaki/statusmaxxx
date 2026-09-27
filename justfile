default:
    @just --list

# Format, lint, and test
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Open the configuration TUI from source
config:
    cargo run -- config

# Render a line for <host> from the session JSON it last sent
render host:
    cargo run -q -- render --host {{host}} < ~/.cache/statusmaxxx/payloads/{{host}}.json

# Tag a release; the dist workflow builds and publishes it
release version:
    cargo set-version {{version}}
    cargo check -q
    git commit -am "chore: release v{{version}}"
    git tag "v{{version}}"
    git push
    git push origin "v{{version}}"
