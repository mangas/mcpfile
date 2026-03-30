# mcpfile

Declarative MCP server manager. Define Docker-based MCP servers in a TOML config — mcpfile handles container lifecycle and secret injection from AWS SSM Parameter Store.

## Install

```bash
cargo install --path .
```

## Quick start

Create `~/.config/mcpfile/config.toml`:

```toml
[defaults]
aws_region = "eu-west-3"
aws_profile = "infra"

[services.grafana]
image = "mcp/grafana:latest"
transport = "sse"
container_port = 8000
env = { GRAFANA_URL = "https://grafana.example.com" }
secrets = { GRAFANA_TOKEN = "/mcpfile/grafana/token" }

[services.tldv]
image = "tldv-mcp-server:latest"
transport = "stdio"
secrets = { TLDV_API_KEY = "/mcpfile/tldv/api-key" }
```

Authenticate with AWS, then start services:

```bash
aws login --profile infra

mcpfile up grafana
# grafana is running on http://localhost:54321

mcpfile up tldv --bridge
# tldv listening on /tmp/mcpfile-tldv-a1b2c3d4.sock

mcpfile status
# SERVICE         STATUS     ENDPOINT
# grafana         running    http://localhost:54321
# tldv            running    -

mcpfile down grafana
mcpfile down tldv
```

## Commands

| Command | Description |
|---------|-------------|
| `mcpfile up <service>` | Start a service. SSE: detached with port. Stdio: foreground. |
| `mcpfile up <service> --bridge` | Stdio over Unix socket (detached, prints socket path). Each call creates an independent instance. |
| `mcpfile up <service> --refresh` | Re-fetch secrets from SSM before starting. |
| `mcpfile up <service> --force` | Stop and recreate if already running. |
| `mcpfile down <service>` | Stop all instances of a service. |
| `mcpfile status` | Show all configured services with running state and endpoint. |
| `mcpfile pull-secrets` | Fetch and cache all secrets. |
| `mcpfile install-skill` | Install Claude Code skill to `~/.claude/skills/mcpfile/`. |
| `mcpfile completions <shell>` | Generate shell completions. |

Global options: `-c <path>` or `--config <path>` to override the config file location.

## Config reference

```toml
[defaults]
aws_region = "eu-west-3"       # default AWS region for SSM calls
aws_profile = "infra"          # default AWS CLI profile

[services.<name>]
image = "org/image:tag"        # Docker image
transport = "sse"              # "sse" (default) or "stdio"
container_port = 8000          # required for SSE, omit for stdio
env = { KEY = "value" }        # static env vars
secrets = { ENV = "/ssm/path" }# env vars fetched from SSM Parameter Store
command = ["arg1", "arg2"]     # optional CMD override
aws_profile = "other"          # per-service AWS profile override
aws_region = "us-east-1"       # per-service AWS region override
```

### Transports

**SSE** (default): Container runs detached. Docker assigns an ephemeral host port, printed on start. Suitable for HTTP-based MCP servers.

**Stdio**: Container's stdin/stdout are the MCP transport.
- Without `--bridge`: runs in the foreground, attached to your terminal.
- With `--bridge`: runs detached with a Unix socket in `/tmp`. Multiple agents can each run `mcpfile up <svc> --bridge` to get independent instances, each with its own container and socket.

### Secrets

Secrets are fetched from AWS SSM Parameter Store via `aws ssm get-parameter --with-decryption` and passed to the container as environment variables (`-e KEY=val`). They are never written to env files.

Cached at `~/.cache/mcpfile/<service>/<ENV_VAR>` with 1-hour TTL. Use `--refresh` to force re-fetch, or `pull-secrets` to pre-cache all secrets.

AWS authentication: run `aws login --profile <profile>` before using mcpfile. On expired credentials, mcpfile prints: `AWS credentials expired. Run: aws login --profile <profile>`.

## Shell completions

### Fish

```bash
mcpfile completions fish > ~/.config/fish/completions/mcpfile.fish
```

Fish completions include dynamic service name completion (tab-completes configured service names for `up` and `down`).

### Bash

```bash
mcpfile completions bash > ~/.local/share/bash-completion/completions/mcpfile
# or source it directly:
source <(mcpfile completions bash)
```

### Zsh

```bash
mcpfile completions zsh > ~/.zfunc/_mcpfile
# ensure ~/.zfunc is in your fpath (add to .zshrc before compinit):
# fpath=(~/.zfunc $fpath)
# autoload -Uz compinit && compinit
```

## Claude Code integration

Install the Claude Code skill so agents know about mcpfile:

```bash
mcpfile install-skill
```

This writes a skill file to `~/.claude/skills/mcpfile/SKILL.md`. Agents can then use `/mcpfile` or auto-detect when MCP server management is relevant.
