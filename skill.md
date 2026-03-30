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
mcpfile status                       # show all services with running state and endpoint
mcpfile up <service>                 # start a service (SSE: detached, stdio: foreground)
mcpfile up <service> --bridge        # stdio over Unix socket (detached, prints socket path)
mcpfile up <service> --refresh       # re-fetch secrets before starting
mcpfile up <service> --force         # stop and recreate if already running
mcpfile down <service>               # stop all instances of a service
mcpfile install-skill                # install Claude Code skill to ~/.claude/skills/
mcpfile completions fish             # generate shell completions (fish/bash/zsh)
mcpfile -c <path> status             # use a custom config file
```

## Config format

```toml
[defaults]
aws_region = "eu-west-3"
aws_profile = "infra"

[services.<name>]
image = "mcp/server:latest"
transport = "sse"           # sse (default, detached with port) | stdio (foreground or --bridge)
container_port = 8000       # required for sse, omit for stdio
env = { KEY = "value" }     # static env vars
secrets = { ENV_VAR = "/ssm/param/path" }  # fetched from AWS SSM
command = ["arg1", "arg2"]  # optional CMD override
aws_profile = "override"   # optional per-service override
aws_region = "override"    # optional per-service override
```

## How it works

- **SSE transport**: creates container with ephemeral host port, prints `<service> is running on http://localhost:<port>`
- **Stdio transport**: runs foreground with inherited stdin/stdout by default
- **Stdio + `--bridge`**: spawns a background bridge process per invocation, each with its own container and temp Unix socket in `/tmp`. Multiple agents can run `mcpfile up <svc> --bridge` independently.
- `down` stops all instances of a service (label-based)
- Secrets are cached at `~/.cache/mcpfile/<service>/` with 1hr TTL
- AWS auth: user must run `aws login --profile <profile>` beforehand
- Uses bollard Docker SDK (no CLI shelling for Docker)

## Source

The mcpfile CLI source is at `~/git/mcpfile`.
