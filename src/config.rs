use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(PathBuf),
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("unknown service: {0}")]
    UnknownService(String),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    #[default]
    Sse,
    Stdio,
}

#[derive(Debug, Deserialize)]
pub struct Defaults {
    pub aws_region: String,
    pub aws_profile: String,
}

#[derive(Debug, Deserialize)]
pub struct ServiceConfig {
    pub image: String,
    #[serde(default)]
    pub transport: Transport,
    pub container_port: Option<u16>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub secrets: HashMap<String, String>,
    pub aws_profile: Option<String>,
    pub aws_region: Option<String>,
    #[serde(default)]
    pub command: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub defaults: Defaults,
    pub services: HashMap<String, ServiceConfig>,
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => resolve_config_path()?,
        };

        if !path.exists() {
            return Err(ConfigError::NotFound(path));
        }

        let contents = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn service(&self, name: &str) -> Result<&ServiceConfig, ConfigError> {
        self.services
            .get(name)
            .ok_or_else(|| ConfigError::UnknownService(name.to_string()))
    }

    pub fn aws_profile<'a>(&'a self, service: &'a ServiceConfig) -> &'a str {
        service
            .aws_profile
            .as_deref()
            .unwrap_or(&self.defaults.aws_profile)
    }

    pub fn aws_region<'a>(&'a self, service: &'a ServiceConfig) -> &'a str {
        service
            .aws_region
            .as_deref()
            .unwrap_or(&self.defaults.aws_region)
    }
}

fn resolve_config_path() -> Result<PathBuf, ConfigError> {
    if let Ok(path) = std::env::var("MCPFILE_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    let home = std::env::var("HOME")
        .map_err(|_| ConfigError::NotFound(PathBuf::from("~/.config/mcpfile/config.toml")))?;

    Ok(PathBuf::from(home).join(".config/mcpfile/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_full_config() {
        let toml_str = r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.grafana]
image = "mcp/grafana:latest"
transport = "sse"
container_port = 8080
env = { GRAFANA_URL = "https://grafana.example.com" }
secrets = { API_KEY = "/infra/mcp/key" }

[services.tldv]
image = "mcp/tldv:latest"
container_port = 3000
aws_profile = "prod"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.defaults.aws_region, "us-east-1");
        assert_eq!(config.defaults.aws_profile, "dev");
        assert_eq!(config.services.len(), 2);

        let grafana = config.service("grafana").unwrap();
        assert_eq!(grafana.image, "mcp/grafana:latest");
        assert!(matches!(grafana.transport, Transport::Sse));
        assert_eq!(grafana.container_port, Some(8080));
        assert_eq!(
            grafana.env.get("GRAFANA_URL").unwrap(),
            "https://grafana.example.com"
        );
        assert_eq!(grafana.secrets.get("API_KEY").unwrap(), "/infra/mcp/key");

        let tldv = config.service("tldv").unwrap();
        assert_eq!(tldv.aws_profile.as_deref(), Some("prod"));
        assert_eq!(config.aws_profile(tldv), "prod");
        assert_eq!(config.aws_region(tldv), "us-east-1");
    }

    #[test]
    fn parse_real_config_format() {
        let toml_str = r#"
[defaults]
aws_region = "eu-west-3"
aws_profile = "infra"

[services.grafana]
image = "mcp/grafana:latest"
transport = "sse"
container_port = 8000
env = { GRAFANA_URL = "https://grafana.example.com" }
secrets = { GRAFANA_SERVICE_ACCOUNT_TOKEN = "/mcpfile/grafana/service-account-token" }

[services.tldv]
image = "tldv-mcp-server:latest"
transport = "stdio"
secrets = { TLDV_API_KEY = "/mcpfile/tldv/api-key" }
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.services.len(), 2);

        let grafana = config.service("grafana").unwrap();
        assert_eq!(grafana.container_port, Some(8000));
        assert!(matches!(grafana.transport, Transport::Sse));

        let tldv = config.service("tldv").unwrap();
        assert!(matches!(tldv.transport, Transport::Stdio));
        assert_eq!(tldv.container_port, None);
        assert_eq!(
            tldv.secrets.get("TLDV_API_KEY").unwrap(),
            "/mcpfile/tldv/api-key"
        );
    }

    #[test]
    fn unknown_service_returns_error() {
        let toml_str = r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.grafana]
image = "mcp/grafana:latest"
container_port = 8080
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.service("nonexistent").is_err());
    }

    #[test]
    fn transport_defaults_to_sse() {
        let toml_str = r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.test]
image = "test:latest"
container_port = 8080
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let svc = config.service("test").unwrap();
        assert!(matches!(svc.transport, Transport::Sse));
    }

    #[test]
    fn stdio_transport() {
        let toml_str = r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.test]
image = "test:latest"
transport = "stdio"
container_port = 8080
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let svc = config.service("test").unwrap();
        assert!(matches!(svc.transport, Transport::Stdio));
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        write!(
            f,
            r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.test]
image = "test:latest"
container_port = 8080
"#
        )
        .unwrap();

        let config = Config::load(Some(&path)).unwrap();
        assert_eq!(config.services.len(), 1);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = Config::load(Some(Path::new("/nonexistent/config.toml")));
        assert!(result.is_err());
    }

    #[test]
    fn aws_profile_falls_back_to_defaults() {
        let toml_str = r#"
[defaults]
aws_region = "eu-west-1"
aws_profile = "default-profile"

[services.test]
image = "test:latest"
container_port = 8080
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let svc = config.service("test").unwrap();
        assert_eq!(config.aws_profile(svc), "default-profile");
        assert_eq!(config.aws_region(svc), "eu-west-1");
    }

    #[test]
    fn empty_env_and_secrets_default() {
        let toml_str = r#"
[defaults]
aws_region = "us-east-1"
aws_profile = "dev"

[services.test]
image = "test:latest"
container_port = 8080
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let svc = config.service("test").unwrap();
        assert!(svc.env.is_empty());
        assert!(svc.secrets.is_empty());
    }
}
