# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace with three primary crates under `crates/`:
- `crates/synthris-core`: core simulation/engine logic.
- `crates/synthris-cli`: CLI for local generation, perf baselines, and profiling.
- `crates/synthris-ris-agent`: RIS polling agent and job handling.

Top-level `src/` contains shared/legacy code; `docs/` holds documentation; `data/` contains sample inputs/assets; `profiles/` stores profiling artifacts; `out/` is used for generated outputs. The `old/` directory contains legacy code/tests and is not part of the active workspace.

## Build, Test, and Development Commands

Key commands (from `README.md`):
- `mise install`: install toolchain versions.
- `cargo test`: run all workspace tests.
- `cargo run -p synthris-cli -- generate -h`: list CLI options.
- `cargo run -p synthris-cli -- generate --out /tmp/synthris-out ...`: generate images locally.
- `cargo run -p synthris-ris-agent`: run the RIS agent.
- `cargo run -p synthris-cli -- perf baseline ...`: perf baseline run.
- `cargo run -p synthris-cli -- perf profile ... --profile-out target/perf`: flamegraph/profile output.

## Coding Style & Naming Conventions

Rust edition is 2024 (`Cargo.toml`). Follow existing Rust conventions in `crates/*/src`:
- `snake_case` for modules/functions/variables.
- `UpperCamelCase` for types.

No repo-specific rustfmt config was found. If formatting is needed, use default rustfmt via `cargo fmt`.

## Testing Guidelines

Tests are primarily inline `#[test]` modules inside crate sources (e.g. `crates/synthris-core/src/*`, `crates/synthris-cli/src/main.rs`, `crates/synthris-ris-agent/src/*`). Run them with `cargo test`. There is no dedicated `tests/` directory in active code; `old/src/tests/` is legacy.

## Commit & Pull Request Guidelines

Recent commit subjects are short, imperative, and sometimes use a scope prefix (e.g. `ci: ...`, `repo: ...`). Keep messages concise and descriptive.

For PRs, include:
- A short summary of behavior change.
- Relevant commands run (e.g. `cargo test`).
- Links to related issues/tasks and any deployment notes.

## Security & Configuration Tips

For the RIS agent, copy `crates/synthris-ris-agent/.env.example` to `.env` and set required variables (`RIS_API_BASE_URL`, `RIS_API_KEY`, `RISFW_VERSION`, `RISHW_VERSION`). Optional defaults are documented in `README.md`.
