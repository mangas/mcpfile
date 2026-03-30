use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use mcpfile::{bridge, config, docker, secrets, skill};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "mcpfile", about = "Declarative MCP server manager")]
struct Cli {
    /// Path to config file
    #[arg(short = 'c', long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a service
    Up {
        service: String,
        /// Force re-fetch secrets from SSM
        #[arg(long)]
        refresh: bool,
        /// Stop and recreate if already running
        #[arg(long)]
        force: bool,
        /// Run stdio service over a Unix socket bridge
        #[arg(long)]
        bridge: bool,
        /// Internal: socket path for bridge subprocess
        #[arg(long, hide = true)]
        bridge_socket: Option<PathBuf>,
        /// Internal: container name for bridge subprocess
        #[arg(long, hide = true)]
        bridge_name: Option<String>,
    },
    /// Stop a service
    Down { service: String },
    /// Show status of all services
    Status,
    /// Fetch and cache all secrets
    PullSecrets,
    /// Install Claude Code skill to ~/.claude/skills/mcpfile/
    InstallSkill,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
    /// List configured service names
    #[command(hide = true)]
    ListServices,
}

fn cache_root() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".cache/mcpfile"))
}

fn generate_fish_completions() {
    print!(
        r#"# mcpfile fish completions
complete -c mcpfile -f

# Global options
complete -c mcpfile -s c -l config -d 'Path to config file' -r -F
complete -c mcpfile -s h -l help -d 'Print help'

# Subcommands
complete -c mcpfile -n __fish_use_subcommand -a up -d 'Start a service'
complete -c mcpfile -n __fish_use_subcommand -a down -d 'Stop a service'
complete -c mcpfile -n __fish_use_subcommand -a status -d 'Show status of all services'
complete -c mcpfile -n __fish_use_subcommand -a pull-secrets -d 'Fetch and cache all secrets'
complete -c mcpfile -n __fish_use_subcommand -a install-skill -d 'Install Claude Code skill'
complete -c mcpfile -n __fish_use_subcommand -a completions -d 'Generate shell completions'

# up flags
complete -c mcpfile -n '__fish_seen_subcommand_from up' -l refresh -d 'Force re-fetch secrets'
complete -c mcpfile -n '__fish_seen_subcommand_from up' -l force -d 'Stop and recreate if running'
complete -c mcpfile -n '__fish_seen_subcommand_from up' -l bridge -d 'Run stdio over Unix socket'

# Dynamic service names for up/down
complete -c mcpfile -n '__fish_seen_subcommand_from up down' -xa '(mcpfile list-services 2>/dev/null)'

# completions shell argument
complete -c mcpfile -n '__fish_seen_subcommand_from completions' -xa 'bash fish zsh elvish powershell'
"#
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Sync commands that don't need config/docker/aws
    match &cli.command {
        Commands::InstallSkill => return skill::install(),
        Commands::Completions { shell } => {
            if *shell == clap_complete::Shell::Fish {
                generate_fish_completions();
            } else {
                clap_complete::generate(
                    *shell,
                    &mut Cli::command(),
                    "mcpfile",
                    &mut std::io::stdout(),
                );
            }
            return Ok(());
        }
        _ => {}
    }

    let cfg = config::Config::load(cli.config.as_deref()).context("failed to load config")?;
    let docker = docker::BollardClient::new()?;
    let aws = secrets::RealAwsClient;

    match cli.command {
        Commands::ListServices => {
            let mut names: Vec<&String> = cfg.services.keys().collect();
            names.sort();
            for name in names {
                println!("{name}");
            }
        }
        Commands::Up {
            service,
            refresh,
            force,
            bridge: use_bridge,
            bridge_socket,
            bridge_name,
        } => {
            let svc = cfg.service(&service)?;
            let cache = cache_root()?;
            let resolved_secrets =
                secrets::resolve_secrets(&aws, &cfg, &service, svc, &cache, refresh).await?;

            // Internal: we are the bridge subprocess
            if let (Some(sock), Some(cname)) = (bridge_socket, bridge_name) {
                return bridge::run(
                    &docker,
                    &service,
                    &cname,
                    &sock,
                    svc,
                    &svc.env,
                    &resolved_secrets,
                )
                .await;
            }

            if use_bridge {
                anyhow::ensure!(
                    matches!(svc.transport, config::Transport::Stdio),
                    "--bridge is only supported for stdio services"
                );
                let sock = bridge::spawn(&service, cli.config.as_deref())?;
                println!("{service} listening on {}", sock.display());
            } else {
                match svc.transport {
                    config::Transport::Sse => {
                        docker::up_sse(&docker, &service, svc, &svc.env, &resolved_secrets, force)
                            .await?;
                    }
                    config::Transport::Stdio => {
                        docker::up_foreground(&docker, &service, svc, &svc.env, &resolved_secrets)
                            .await?;
                    }
                }
            }
        }
        Commands::Down { service } => {
            docker::down(&docker, &service).await?;
        }
        Commands::Status => {
            docker::status(&docker, &cfg).await?;
        }
        Commands::PullSecrets => {
            let cache = cache_root()?;
            secrets::pull_all_secrets(&aws, &cfg, &cache).await?;
        }
        Commands::InstallSkill | Commands::Completions { .. } => unreachable!(),
    }

    Ok(())
}
