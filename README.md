# Synthris

Synthetic bacterial colony image generation for RIS testing.

## Features

- Synthetic plate image generation with growth over time
- RIS agent that polls jobs and uploads generated captures
- Job-name overrides for CFU, organism, seed, and pacing
- Rust CLI for local generation, perf baselines, and profiling
- Fly.io deployment support

## Toolchain (mise)

```bash
mise install
mise ls
rustc --version
cargo --version
flyctl version
```

## Environment

```bash
cp crates/synthris-ris-agent/.env.example .env
```

Required for agent runtime:

- `RIS_API_BASE_URL`
- `RIS_API_KEY`
- `RISFW_VERSION`
- `RISHW_VERSION`

Optional:

- `POLL_INTERVAL_SECONDS` (default `2.0`)
- `DEFAULT_TEMPERATURE_C` (default `22.0`)
- `DEFAULT_SCALE_FACTOR` (default `1.0`, clamped to `0.05..=1.0`)

## Quickstart

```bash
# Generate images locally
cargo run -p synthris-cli -- generate \
  --out /tmp/synthris-out \
  --organism morrow \
  --cfu 50-80 \
  --illumination backlit \
  --duration 2d \
  --step 6h

# Discover all CLI args
cargo run -p synthris-cli -- generate -h
```

Common flags:

- `--out`, `--output-format`, `--fps`
- `--organism`, `--cfu`, `--illumination`, `--plate-profile`, `--background-mode`
- `--duration`, `--step`, `--start-after`, `--temperature-c`
- `--seed`, `--width`, `--height`, `--profile-dir`

## RIS Agent

Run:

```bash
cargo run -p synthris-ris-agent
```

Job-name parameter docs (canonical):

- `crates/synthris-ris-agent/README.md`

## Workspace Commands

```bash
# Tests
cargo test

# Perf baseline
cargo run -p synthris-cli -- perf baseline \
  --out /tmp/synthris-perf \
  --organism morrow \
  --illumination backlit \
  --plate-profile petri-default \
  --cfu 50 \
  --runs 3

# Perf flamegraph/profile output
cargo run -p synthris-cli -- perf profile \
  --out /tmp/synthris-perf-profile \
  --organism morrow \
  --illumination backlit \
  --plate-profile petri-default \
  --cfu 150 \
  --profile-out target/perf
```

## Deployment

```bash
flyctl deploy -c crates/synthris-ris-agent/fly.staging.toml --dockerfile crates/synthris-ris-agent/Dockerfile
flyctl deploy -c crates/synthris-ris-agent/fly.prod.toml --dockerfile crates/synthris-ris-agent/Dockerfile
```
