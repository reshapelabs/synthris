# synthris-ris-agent

RIS polling agent for Synthris.

## What It Does

- Polls RIS for pending jobs
- Resolves generation params from job metadata + job name overrides
- Renders frames with `synthris-core`
- Uploads one JPEG per rendered frame per well/preset

## Run

```bash
cargo run -p synthris-ris-agent
```

Env template:

```bash
cp crates/synthris-ris-agent/.env.example .env
```

## Deployment

```bash
flyctl deploy -c crates/synthris-ris-agent/fly.staging.toml --dockerfile crates/synthris-ris-agent/Dockerfile
flyctl deploy -c crates/synthris-ris-agent/fly.prod.toml --dockerfile crates/synthris-ris-agent/Dockerfile
```

Required env vars:

- `RIS_API_BASE_URL`
- `RIS_API_KEY`
- `RISFW_VERSION`
- `RISHW_VERSION`

Optional env vars:

- `POLL_INTERVAL_SECONDS` (default `2.0`)
- `DEFAULT_TEMPERATURE_C` (default `22.0`)
- `DEFAULT_SCALE_FACTOR` (default `1.0`, clamped to `0.05..=1.0`)

## Job Name Overrides

The agent scans `job.name` for `key=value` tokens (case-insensitive).

Supported keys:

- `cfu=<n>`: exact CFU, e.g. `cfu=75`
- `cfu=<min>-<max>`: CFU range, e.g. `cfu=50-100`
- `col=...`: legacy alias for `cfu` (last key wins if both are present)
- `organism=<id>` or `org=<id>`: organism profile id, e.g. `organism=morrow`
- `seed=<u64>`: deterministic seed override
- `pace=<bool>`: whether to align each time index to capture interval marks

`pace` accepted values:

- true: `1`, `true`, `yes`, `on`
- false: `0`, `false`, `no`, `off`

Defaults when missing:

- `cfu`: `50`
- `organism`: `morrow`
- `pace`: `false`
- `seed`: derived from job/plate identifiers
- temperature: job setpoint if present, else `DEFAULT_TEMPERATURE_C`

Validation notes:

- malformed tokens for supported keys fail parsing
- invalid organism id characters are rejected
- unknown organism profile ids fail fast before rendering

## Examples

- `staging run cfu=60-80 organism=morrow`
- `smoke test cfu=45 org=ecoli seed=123`
- `timed capture cfu=75 organism=morrow pace=1`
