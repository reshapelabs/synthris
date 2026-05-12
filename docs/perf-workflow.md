# Perf Workflow

Use release mode for realistic throughput checks.

## 1) Baseline timing (repeatable)

```bash
cargo run --release -p synthris-cli -- perf baseline \
  --out /tmp/synthris-perf \
  --organism morrow \
  --illumination backlit \
  --cfu 50 \
  --plate-profile petri-default \
  --duration 2d --step 1h --runs 3
```

Outputs per-run timing and summary (avg/min/max ms).

## 2) CPU profile + flamegraph

```bash
cargo run --release -p synthris-cli -- perf profile \
  --out /tmp/synthris-perf-profile \
  --organism morrow \
  --illumination backlit \
  --cfu 50 \
  --plate-profile petri-default \
  --duration 2d --step 1h \
  --profile-out target/perf
```

Artifacts:
- `target/perf/synthris-<ts>.svg` flamegraph
- `target/perf/synthris-<ts>.pb` pprof profile

## 3) Stage timing traces (no print timing)

```bash
RUST_LOG=synthris_core=info,synthris_cli=info \
  cargo run --release -p synthris-cli -- generate \
  -o /tmp/synthris-out -g morrow -i backlit -c 50 -p petri-default -d 2d -s 1h
```

Engine logs a stage summary including lookup, seeding, background, paint, encode, and total ms.

## Optional: turbojpeg build

The default build uses `image` JPEG encoding. Build with turbojpeg feature to use turbojpeg automatically:

```bash
cargo run --release -p synthris-cli --features synthris-core/turbojpeg -- perf baseline \
  --out /tmp/synthris-perf-turbojpeg \
  --organism morrow \
  --illumination backlit \
  --cfu 50 \
  --plate-profile petri-default \
  --runs 3
```
