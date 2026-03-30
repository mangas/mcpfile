# mcpfile

Declarative MCP server manager. TOML config defines Docker-based MCP servers; mcpfile handles container lifecycle and secret injection from AWS SSM.

## Project structure

```
src/
  lib.rs           # crate root, declares all modules
  main.rs          # CLI entrypoint (clap + tokio)
  config.rs        # TOML config parsing and types
  secrets.rs       # AwsClient trait, SSM fetch, caching
  docker.rs        # DockerClient trait, BollardClient, container lifecycle
  piped_io.rs      # async Unix socket ↔ (AsyncRead, AsyncWrite) bridge
  bridge.rs        # orchestrator: composes DockerClient + PipedIo for stdio services
  skill.rs         # Claude Code skill installer
```

## Architecture

All side effects are abstracted behind traits for testability:

- **`DockerClient`** (docker.rs): create/start/stop/attach/inspect/list/wait. `BollardClient` is the real impl using bollard SDK. `MockDockerClient` for unit tests.
- **`AwsClient`** (secrets.rs): `fetch_ssm_parameter`. `RealAwsClient` shells out to `aws` CLI via `tokio::process::Command`. `MockAwsClient` for unit tests.
- **`PipedIo`** (piped_io.rs): bridges a Unix socket to any `(AsyncRead, AsyncWrite)` pair. No Docker knowledge. Tested with `tokio::io::duplex`.

Intermediate types (`CreateContainerParams`, `ContainerInfo`, `AttachStreams`) decouple the `DockerClient` trait from bollard internals so mocks don't need bollard imports.

## Key design decisions

- **bollard + tokio** for Docker (no CLI shelling). AWS SSM still shells out to `aws` CLI (no SDK).
- **`--bridge` flag** on `up` for stdio services: spawns a background bridge process with a unique temp socket in `/tmp` and a unique container name (`mcpfile-<service>-<suffix>`). Multiple agents can each get independent instances.
- **Label-based `down`**: stops all containers matching `mcpfile.service=<name>` (handles multiple bridge instances).
- Secrets cached at `~/.cache/mcpfile/<service>/<ENV_VAR>` with 1hr TTL, mode 0600/0700.
- Config path: `--config` / `-c` > `$MCPFILE_CONFIG` > `~/.config/mcpfile/config.toml`.

## Build & test

```bash
cargo build
cargo test                    # 31 unit tests, no Docker/AWS needed
cargo test -- --ignored       # integration tests requiring Docker
cargo clippy -- -D warnings
```

## Conventions

- `anyhow` for application errors, `thiserror` for library errors
- `impl Trait` in function args, explicit types in returns
- `#[must_use]` where ignoring return value is likely a bug
- No `.unwrap()`/`.expect()` in library code; allowed in tests/main
- Flat control flow: early returns, `continue` guards, iterator chains over nested `if let`/`if`/`match`
- `clippy` warnings as errors
- Traits for side effects with mock impls for tests
