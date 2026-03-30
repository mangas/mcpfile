# mcpfile

Declarative MCP server manager. TOML config defines MCP servers; mcpfile handles Docker lifecycle and secret injection from AWS SSM.

## Project structure

```
src/
  main.rs          # CLI entrypoint (clap)
  config.rs        # TOML config parsing
  secrets.rs       # SSM fetch + caching
  docker.rs        # Container lifecycle
```

## AWS authentication

mcpfile relies on the `aws` CLI for all AWS operations (no SDK). The `aws_profile` config setting is passed as `--profile` on every call.

Users authenticate with `aws login --profile <profile>` before running mcpfile. This is an AWS CLI v2 built-in that uses browser-based console auth with auto-refreshing tokens. On credential expiry, mcpfile should detect the non-zero exit and print: `AWS credentials expired. Run: aws login --profile <profile>`

## Build & install

```bash
cargo install --path .
```

## Conventions

- Shell out to `docker` and `aws` CLIs — no AWS SDK or Docker SDK dependency
- Secrets are never written to env files; passed directly via `docker run -e`
- All containers named `mcpfile-<service>` with labels `mcpfile.managed=true` and `mcpfile.service=<name>`
- Host ports are ephemeral (auto-assigned); printed on `up`
- See PLAN.md for full design
