# mcpfile

Declarative MCP server manager. Define MCP servers in a TOML config, mcpfile handles Docker lifecycle and secret injection from AWS SSM.

## Usage

```bash
mcpfile up grafana        # fetch secrets from SSM, start container, print assigned port
mcpfile down grafana      # stop and remove container
mcpfile status            # list all services with state and port
mcpfile pull-secrets      # fetch all secrets to local cache
```

## Config

Default location: `~/.config/mcpfile/config.toml`

Override with `MCPFILE_CONFIG` env var or `--config` flag.

```toml
[defaults]
aws_region = "eu-west-3"
aws_profile = "infra"          # used for all SSM calls; override per-service if needed

[services.grafana]
image = "mcp/grafana-mcp-server:latest"
transport = "sse"              # sse (default) | stdio
container_port = 8080          # port the server listens on inside the container
env = { GRAFANA_URL = "https://grafana.example.com" }
secrets = { GRAFANA_API_KEY = "/infra/mcp/grafana-api-key" }

[services.tldv]
image = "mcp/tldv-mcp-server:latest"
container_port = 8080
secrets = { TLDV_API_KEY = "/infra/mcp/tldv-api-key" }
```

- `env`: static environment variables passed to the container
- `secrets`: map of `ENV_VAR_NAME = "SSM parameter path"`. Resolved at `up` time and injected as env vars.
- `transport`: `stdio` runs with `docker run -i --rm`, `sse` runs detached with port mapping
- `container_port`: the port the MCP server listens on inside the container. The host port is auto-assigned (ephemeral) and printed on `up`.
- `aws_profile` / `aws_region`: optional per-service overrides; defaults to `[defaults]` values.

## Design

### AWS authentication

mcpfile shells out to the `aws` CLI and relies on its credential chain. The `aws_profile` setting in config maps to `--profile` on all AWS calls.

Authenticate before using mcpfile:
```bash
aws login --profile infra
```

`aws login` is an AWS CLI v2 built-in that opens a browser for console-based authentication, stores temporary credentials with an auto-refreshing token locally. No SSO IdP configuration needed — just an AWS account with console access.

If credentials are expired or missing, mcpfile should detect this (non-zero exit from `aws`) and print a clear message: `AWS credentials expired. Run: aws login --profile <profile>`

### Secret handling

- `up` fetches secrets on demand from SSM via `aws ssm get-parameter --with-decryption --profile <aws_profile> --region <aws_region>`
- `pull-secrets` fetches all secrets and caches them to `~/.cache/mcpfile/<service>/<ENV_VAR_NAME>` (mode 0600, directory 0700)
- `up` uses cached secrets if available and less than 1 hour old; pass `--refresh` to force re-fetch
- Secrets are never written to Docker env files — passed via `docker run -e KEY=val` args

### Docker lifecycle

### Container naming

All containers use the prefix `mcpfile-` followed by the service name:
- `mcpfile-grafana`
- `mcpfile-tldv`

This makes them discoverable via `docker ps --filter name=mcpfile-` and ensures `mcpfile status` can reliably identify which containers it owns.

Docker labels are also applied for additional filtering:
- `mcpfile.service=<name>`
- `mcpfile.managed=true`

**sse transport** (default):
```
docker run -d --rm --name mcpfile-<service> \
  --label mcpfile.service=<service> \
  --label mcpfile.managed=true \
  -e KEY1=val1 -e KEY2=val2 \
  -p 0:<container_port> \
  <image>
```
Runs detached. Host port is auto-assigned by Docker (`-p 0:<container_port>`). After starting, mcpfile queries the assigned port via `docker port mcpfile-<service>` and prints it:

```
$ mcpfile up grafana
grafana is running on http://localhost:54321
```

**stdio transport**:
```
docker run -i --rm --name mcpfile-<service> \
  --label mcpfile.service=<service> \
  --label mcpfile.managed=true \
  -e KEY1=val1 -e KEY2=val2 \
  <image>
```
Container stays in foreground attached to stdin/stdout.

### `down`

```
docker stop mcpfile-<service>
```

### `status`

Lists all services from config and their state. Uses `docker ps -a --filter label=mcpfile.managed=true` to find owned containers.

```
$ mcpfile status
SERVICE   STATUS    PORT
grafana   running   http://localhost:54321
tldv      stopped   -
```

## Implementation

### Language

Rust. Installed via `cargo install --path .`

### Dependencies

- `clap` — CLI arg parsing
- `toml` — config parsing
- `serde` / `serde_derive` — deserialization
- `std::process::Command` — shell out to `docker` and `aws` CLIs (no SDK dependency)

### File structure

```
mcpfile/
  Cargo.toml
  src/
    main.rs          # CLI entrypoint, clap setup
    config.rs        # TOML config parsing and types
    secrets.rs       # SSM fetch, caching, cache invalidation
    docker.rs        # Container lifecycle (up/down/ls)
```

### Implementation steps

1. **Config parsing** — define structs for config.toml, deserialize with serde/toml, resolve config path (`--config` > `$MCPFILE_CONFIG` > `~/.config/mcpfile/config.toml`)

2. **Secret resolution** — shell out to `aws ssm get-parameter --name <path> --with-decryption --profile <aws_profile> --region <aws_region> --query Parameter.Value --output text`. Profile comes from service-level `aws_profile`, falling back to `defaults.aws_profile`. Same for region. Cache to `~/.cache/mcpfile/`. Check cache age before fetching.

3. **Docker SSE** (default) — `docker run -d` with `-p 0:<container_port>` for ephemeral host port. After start, query `docker port mcpfile-<service>` and print the URL.

4. **Docker stdio** — `std::process::Command` with stdin/stdout inherited from the parent process, foreground.

5. **Down** — `docker stop mcpfile-<service>`.

6. **Status** — read config for defined services, `docker ps -a --filter label=mcpfile.managed=true --format json` to get state and port mappings. Print table.

7. **Pull-secrets** — iterate all services, fetch all secrets, write to cache.

### Error handling

- Missing/expired AWS credentials: detect non-zero exit from `aws` and print `AWS credentials expired. Run: aws login --profile <profile>`
- Docker not running: detect and report
- SSM parameter not found: report which service/secret failed
- Container already running on `up`: report and skip (or `--force` to recreate)
