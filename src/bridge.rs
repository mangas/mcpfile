use crate::config::ServiceConfig;
use crate::docker::{self, DockerClient};
use crate::piped_io::PipedIo;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::Duration;

fn random_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(std::process::id() as u64);
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("{:08x}", hasher.finish() & 0xFFFF_FFFF)
}

/// Spawn a detached bridge process. Returns the temp socket path.
pub fn spawn(service_name: &str, config_path: Option<&Path>) -> Result<PathBuf> {
    let suffix = random_suffix();
    let sock = std::env::temp_dir().join(format!("mcpfile-{service_name}-{suffix}.sock"));
    let container = format!("mcpfile-{service_name}-{suffix}");

    let exe = std::env::current_exe().context("failed to find mcpfile binary")?;
    let mut args: Vec<String> = Vec::new();

    if let Some(path) = config_path {
        args.extend(["-c".to_string(), path.display().to_string()]);
    }

    args.extend([
        "up".to_string(),
        service_name.to_string(),
        "--bridge-socket".to_string(),
        sock.display().to_string(),
        "--bridge-name".to_string(),
        container,
    ]);

    let log = std::env::temp_dir().join(format!("mcpfile-{service_name}-{suffix}.log"));
    let log_file = std::fs::File::create(&log)?;

    std::process::Command::new(exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(log_file.try_clone()?)
        .stderr(log_file)
        .spawn()
        .context("failed to spawn bridge process")?;

    for _ in 0..50 {
        if sock.exists() {
            return Ok(sock);
        }
        thread::sleep(Duration::from_millis(100));
    }

    bail!("bridge failed to start — check {}", log.display())
}

/// Run the bridge: spawn docker container, bridge its stdio to a Unix socket.
pub async fn run(
    docker_client: &impl DockerClient,
    service_name: &str,
    container_name: &str,
    socket_path: &Path,
    service: &ServiceConfig,
    env_vars: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
) -> Result<()> {
    let (id, streams) = docker::spawn_docker_stdio(
        docker_client,
        container_name,
        service_name,
        service,
        env_vars,
        secrets,
    )
    .await?;

    let piped_io = PipedIo::bind(socket_path).await?;
    piped_io.run(streams.output, streams.input).await?;

    let _ = docker_client.wait_container(&id).await;
    Ok(())
}
