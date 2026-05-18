# Synthris

Synthetic bacterial colony image generation. Produces images that match Reshape Biotech's camera stack.

## Features

- Synthetic plate image generation with growth over time
- Rust CLI for local generation, perf baselines, and profiling

## Toolchain (mise)

```bash
mise trust
mise install
```

## Quickstart (CLI)

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
