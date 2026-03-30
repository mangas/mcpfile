use crate::config::{Config, ServiceConfig};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

const CACHE_TTL: Duration = Duration::from_secs(3600);

#[allow(async_fn_in_trait)]
pub trait AwsClient {
    async fn fetch_ssm_parameter(
        &self,
        profile: &str,
        region: &str,
        parameter_name: &str,
    ) -> Result<String>;
}

pub struct RealAwsClient;

impl AwsClient for RealAwsClient {
    async fn fetch_ssm_parameter(
        &self,
        profile: &str,
        region: &str,
        parameter_name: &str,
    ) -> Result<String> {
        let output = tokio::process::Command::new("aws")
            .args([
                "ssm",
                "get-parameter",
                "--name",
                parameter_name,
                "--with-decryption",
                "--profile",
                profile,
                "--region",
                region,
                "--query",
                "Parameter.Value",
                "--output",
                "text",
            ])
            .output()
            .await?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let is_auth_error = stderr.contains("ExpiredToken")
            || stderr.contains("credentials")
            || stderr.contains("UnrecognizedClientException")
            || stderr.contains("InvalidIdentityToken");

        if is_auth_error {
            bail!("AWS credentials expired. Run: aws login --profile {profile}");
        }

        bail!(
            "failed to fetch secret {parameter_name}: {}",
            stderr.trim()
        );
    }
}

fn read_cached(cache_root: &Path, service: &str, env_var: &str) -> Result<Option<String>> {
    let path = cache_root.join(service).join(env_var);

    if !path.exists() {
        return Ok(None);
    }

    let metadata = fs::metadata(&path)?;
    let modified = metadata.modified()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(CACHE_TTL);

    if age >= CACHE_TTL {
        return Ok(None);
    }

    let value = fs::read_to_string(&path)?;
    Ok(Some(value))
}

fn write_cache(cache_root: &Path, service: &str, env_var: &str, value: &str) -> Result<()> {
    let dir = cache_root.join(service);
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;

    let path = dir.join(env_var);
    fs::write(&path, value)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub async fn resolve_secrets(
    aws: &impl AwsClient,
    config: &Config,
    service_name: &str,
    service: &ServiceConfig,
    cache_root: &Path,
    refresh: bool,
) -> Result<HashMap<String, String>> {
    let profile = config.aws_profile(service);
    let region = config.aws_region(service);
    let mut resolved = HashMap::new();

    for (env_var, param_path) in &service.secrets {
        let cached = if refresh {
            None
        } else {
            read_cached(cache_root, service_name, env_var)?
        };

        let value = match cached {
            Some(v) => v,
            None => {
                let val = aws.fetch_ssm_parameter(profile, region, param_path).await?;
                write_cache(cache_root, service_name, env_var, &val)?;
                val
            }
        };

        resolved.insert(env_var.clone(), value);
    }

    Ok(resolved)
}

pub async fn pull_all_secrets(
    aws: &impl AwsClient,
    config: &Config,
    cache_root: &Path,
) -> Result<()> {
    for (name, service) in &config.services {
        if service.secrets.is_empty() {
            continue;
        }
        eprintln!("Fetching secrets for {name}...");
        resolve_secrets(aws, config, name, service, cache_root, true).await?;
        eprintln!("  Cached {} secret(s)", service.secrets.len());
    }
    Ok(())
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::cell::RefCell;

    pub struct MockAwsClient {
        responses: RefCell<Vec<Result<String>>>,
        calls: RefCell<Vec<(String, String, String)>>,
    }

    impl MockAwsClient {
        pub fn new(responses: Vec<Result<String>>) -> Self {
            Self {
                responses: RefCell::new(responses),
                calls: RefCell::new(Vec::new()),
            }
        }

        pub fn calls(&self) -> Vec<(String, String, String)> {
            self.calls.borrow().clone()
        }
    }

    impl AwsClient for MockAwsClient {
        async fn fetch_ssm_parameter(
            &self,
            profile: &str,
            region: &str,
            parameter_name: &str,
        ) -> Result<String> {
            self.calls.borrow_mut().push((
                profile.to_string(),
                region.to_string(),
                parameter_name.to_string(),
            ));
            let mut responses = self.responses.borrow_mut();
            anyhow::ensure!(!responses.is_empty(), "MockAwsClient: no more responses");
            responses.remove(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Defaults, ServiceConfig, Transport};
    use mock::MockAwsClient;

    fn test_config() -> Config {
        Config {
            defaults: Defaults {
                aws_region: "us-east-1".into(),
                aws_profile: "dev".into(),
            },
            services: HashMap::from([(
                "svc".into(),
                ServiceConfig {
                    image: "test:latest".into(),
                    transport: Transport::Sse,
                    container_port: Some(8080),
                    env: HashMap::new(),
                    secrets: HashMap::from([("API_KEY".into(), "/path/to/key".into())]),
                    aws_profile: None,
                    aws_region: None,
                    command: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn write_and_read_cache() {
        let dir = tempfile::tempdir().unwrap();
        write_cache(dir.path(), "svc", "API_KEY", "secret123").unwrap();

        let cached = read_cached(dir.path(), "svc", "API_KEY").unwrap();
        assert_eq!(cached, Some("secret123".to_string()));
    }

    #[test]
    fn cache_miss_when_not_exists() {
        let dir = tempfile::tempdir().unwrap();
        let cached = read_cached(dir.path(), "svc", "MISSING").unwrap();
        assert_eq!(cached, None);
    }

    #[test]
    fn cache_permissions() {
        let dir = tempfile::tempdir().unwrap();
        write_cache(dir.path(), "svc", "KEY", "val").unwrap();

        let file_meta = fs::metadata(dir.path().join("svc/KEY")).unwrap();
        let dir_meta = fs::metadata(dir.path().join("svc")).unwrap();

        assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
        assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn cache_expired_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("svc");
        fs::create_dir_all(&path).unwrap();
        let file_path = path.join("OLD_KEY");
        fs::write(&file_path, "stale").unwrap();

        let two_hours_ago =
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(7200));
        filetime::set_file_mtime(&file_path, two_hours_ago).unwrap();

        let cached = read_cached(dir.path(), "svc", "OLD_KEY").unwrap();
        assert_eq!(cached, None);
    }

    #[tokio::test]
    async fn resolve_fetches_from_ssm_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let aws = MockAwsClient::new(vec![Ok("secret-value".into())]);

        let config = test_config();
        let svc = config.service("svc").unwrap();
        let result = resolve_secrets(&aws, &config, "svc", svc, dir.path(), false)
            .await
            .unwrap();

        assert_eq!(result.get("API_KEY").unwrap(), "secret-value");
        let cached = read_cached(dir.path(), "svc", "API_KEY").unwrap();
        assert_eq!(cached, Some("secret-value".to_string()));
    }

    #[tokio::test]
    async fn resolve_uses_fresh_cache() {
        let dir = tempfile::tempdir().unwrap();
        write_cache(dir.path(), "svc", "API_KEY", "cached-value").unwrap();

        let aws = MockAwsClient::new(vec![]);

        let config = test_config();
        let svc = config.service("svc").unwrap();
        let result = resolve_secrets(&aws, &config, "svc", svc, dir.path(), false)
            .await
            .unwrap();

        assert_eq!(result.get("API_KEY").unwrap(), "cached-value");
        assert!(aws.calls().is_empty());
    }

    #[tokio::test]
    async fn resolve_refresh_ignores_cache() {
        let dir = tempfile::tempdir().unwrap();
        write_cache(dir.path(), "svc", "API_KEY", "old-value").unwrap();

        let aws = MockAwsClient::new(vec![Ok("new-value".into())]);

        let config = test_config();
        let svc = config.service("svc").unwrap();
        let result = resolve_secrets(&aws, &config, "svc", svc, dir.path(), true)
            .await
            .unwrap();

        assert_eq!(result.get("API_KEY").unwrap(), "new-value");
        assert_eq!(aws.calls().len(), 1);
    }

    #[tokio::test]
    async fn expired_credentials_error() {
        let dir = tempfile::tempdir().unwrap();
        let aws = MockAwsClient::new(vec![Err(anyhow::anyhow!(
            "AWS credentials expired. Run: aws login --profile dev"
        ))]);

        let config = test_config();
        let svc = config.service("svc").unwrap();
        let result = resolve_secrets(&aws, &config, "svc", svc, dir.path(), false).await;

        let err = result.unwrap_err().to_string();
        assert!(err.contains("AWS credentials expired"));
    }

    #[tokio::test]
    async fn ssm_fetch_passes_correct_args() {
        let dir = tempfile::tempdir().unwrap();
        let aws = MockAwsClient::new(vec![Ok("val".into())]);

        let config = test_config();
        let svc = config.service("svc").unwrap();
        resolve_secrets(&aws, &config, "svc", svc, dir.path(), false)
            .await
            .unwrap();

        let calls = aws.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "dev"); // profile
        assert_eq!(calls[0].1, "us-east-1"); // region
        assert_eq!(calls[0].2, "/path/to/key"); // param
    }

    #[tokio::test]
    async fn pull_all_skips_services_without_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let aws = MockAwsClient::new(vec![Ok("val".into())]);

        let config = Config {
            defaults: Defaults {
                aws_region: "us-east-1".into(),
                aws_profile: "dev".into(),
            },
            services: HashMap::from([
                (
                    "with_secrets".into(),
                    ServiceConfig {
                        image: "img:latest".into(),
                        transport: Transport::Sse,
                        container_port: Some(8080),
                        env: HashMap::new(),
                        secrets: HashMap::from([("KEY".into(), "/path".into())]),
                        aws_profile: None,
                        aws_region: None,
                        command: Vec::new(),
                    },
                ),
                (
                    "no_secrets".into(),
                    ServiceConfig {
                        image: "img:latest".into(),
                        transport: Transport::Sse,
                        container_port: Some(8080),
                        env: HashMap::new(),
                        secrets: HashMap::new(),
                        aws_profile: None,
                        aws_region: None,
                        command: Vec::new(),
                    },
                ),
            ]),
        };

        pull_all_secrets(&aws, &config, dir.path()).await.unwrap();
        assert_eq!(aws.calls().len(), 1);
    }
}
