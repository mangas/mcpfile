---
name: mcpfile
description: Manage Docker-based MCP servers. Use when the user asks about available MCP servers, wants to start/stop MCP services, check MCP status, or needs to know what tools are available via MCP.
---

mcpfile is a CLI that manages Docker-based MCP servers defined in `~/.config/mcpfile/config.toml`.

## Check configured MCP servers

Read the config to see what's available:

```
cat ~/.config/mcpfile/config.toml
```

## Commands

```bash
mcpfile status                  # show all services with running state and port
mcpfile up <service>            # start a service (fetches secrets from SSM, starts Docker container)
mcpfile up <service> --refresh  # re-fetch secrets before starting
mcpfile up <service> --force    # stop and recreate if already running
mcpfile down <service>          # stop a service
mcpfile pull-secrets            # fetch and cache all secrets
mcpfile -c <path> status       # use a custom config file
```

## Config format

```toml
[defaults]
aws_region = "eu-west-3"
aws_profile = "fa"

[services.<name>]
image = "mcp/server:latest"
transport = "sse"           # sse (default, detached with port) | stdio (foreground)
container_port = 8000       # required for sse, omit for stdio
env = { KEY = "value" }     # static env vars
secrets = { ENV_VAR = "/ssm/param/path" }  # fetched from AWS SSM
aws_profile = "override"   # optional per-service override
aws_region = "override"    # optional per-service override
```

## How it works

- **SSE transport**: `docker run -d` with ephemeral host port. After start, prints `<service> is running on http://localhost:<port>`
- **Stdio transport**: `docker run -i` foreground with inherited stdin/stdout
- Containers are named `mcpfile-<service>` with labels `mcpfile.managed=true`
- Secrets are cached at `~/.cache/mcpfile/<service>/` with 1hr TTL
- AWS auth: user must run `aws login --profile <profile>` beforehand

## Source

The mcpfile CLI source is at `~/git/mcpfile`.
