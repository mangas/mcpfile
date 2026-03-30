use crate::config::{Config, ServiceConfig, Transport};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};

// ---------------------------------------------------------------------------
// Intermediate types (decoupled from bollard)
// ---------------------------------------------------------------------------

pub struct CreateContainerParams {
    pub name: String,
    pub image: String,
    pub labels: HashMap<String, String>,
    pub env: Vec<String>,
    pub exposed_ports: Vec<u16>,
    pub stdin_open: bool,
    pub auto_remove: bool,
    pub command: Vec<String>,
}

pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub state: String,
    pub labels: HashMap<String, String>,
    pub ports: Vec<PortMapping>,
}

pub struct PortMapping {
    pub container_port: u16,
    pub host_port: u16,
}

pub struct AttachStreams {
    pub output: Pin<Box<dyn AsyncRead + Send>>,
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
}

// ---------------------------------------------------------------------------
// DockerClient trait
// ---------------------------------------------------------------------------

#[allow(async_fn_in_trait)]
pub trait DockerClient {
    async fn create_container(&self, params: &CreateContainerParams) -> Result<String>;
    async fn start_container(&self, id: &str) -> Result<()>;
    async fn stop_container(&self, id: &str) -> Result<()>;
    async fn remove_container(&self, id: &str, force: bool) -> Result<()>;
    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo>;
    async fn attach_container(&self, id: &str) -> Result<AttachStreams>;
    async fn list_containers_by_label(&self, label: &str, value: &str) -> Result<Vec<ContainerInfo>>;
    async fn wait_container(&self, id: &str) -> Result<i64>;
}

// ---------------------------------------------------------------------------
// BollardClient
// ---------------------------------------------------------------------------

pub struct BollardClient {
    inner: bollard::Docker,
}

impl BollardClient {
    pub fn new() -> Result<Self> {
        let docker = bollard::Docker::connect_with_local_defaults()
            .context("failed to connect to Docker daemon")?;
        Ok(Self { inner: docker })
    }
}

impl DockerClient for BollardClient {
    async fn create_container(&self, params: &CreateContainerParams) -> Result<String> {
        use bollard::container::{Config as ContainerConfig, CreateContainerOptions};
        use bollard::models::{HostConfig, PortBinding};

        let mut port_bindings = HashMap::new();
        let mut exposed_ports = HashMap::new();
        for port in &params.exposed_ports {
            let key = format!("{port}/tcp");
            exposed_ports.insert(key.clone(), HashMap::new());
            port_bindings.insert(
                key,
                Some(vec![PortBinding {
                    host_ip: None,
                    host_port: Some(String::new()), // ephemeral
                }]),
            );
        }

        let host_config = HostConfig {
            auto_remove: Some(params.auto_remove),
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            ..Default::default()
        };

        let config = ContainerConfig {
            image: Some(params.image.clone()),
            labels: Some(params.labels.clone()),
            env: Some(params.env.clone()),
            open_stdin: Some(params.stdin_open),
            attach_stdin: Some(params.stdin_open),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            host_config: Some(host_config),
            cmd: if params.command.is_empty() {
                None
            } else {
                Some(params.command.clone())
            },
            ..Default::default()
        };

        let opts = CreateContainerOptions {
            name: params.name.clone(),
            ..Default::default()
        };

        let resp = self.inner.create_container(Some(opts), config).await?;
        Ok(resp.id)
    }

    async fn start_container(&self, id: &str) -> Result<()> {
        self.inner
            .start_container::<&str>(id, None)
            .await
            .context("failed to start container")?;
        Ok(())
    }

    async fn stop_container(&self, id: &str) -> Result<()> {
        self.inner
            .stop_container(id, None)
            .await
            .context("failed to stop container")?;
        Ok(())
    }

    async fn remove_container(&self, id: &str, force: bool) -> Result<()> {
        use bollard::container::RemoveContainerOptions;
        self.inner
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force,
                    ..Default::default()
                }),
            )
            .await
            .context("failed to remove container")?;
        Ok(())
    }

    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo> {
        let resp = self.inner.inspect_container(id, None).await?;

        let name = resp
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();
        let state_str = resp
            .state
            .as_ref()
            .and_then(|s| s.status.as_ref())
            .map(|s| format!("{s:?}").to_lowercase())
            .unwrap_or_else(|| "unknown".into());
        let labels = resp.config.as_ref()
            .and_then(|c| c.labels.clone())
            .unwrap_or_default();

        let mut ports = Vec::new();
        if let Some(network) = resp.network_settings.as_ref() {
            if let Some(port_map) = network.ports.as_ref() {
                for (key, bindings) in port_map {
                    let container_port = key
                        .split('/')
                        .next()
                        .and_then(|p| p.parse::<u16>().ok())
                        .unwrap_or(0);
                    if let Some(Some(bindings)) = bindings.as_ref().map(Some) {
                        for b in bindings {
                            if let Some(hp) = b.host_port.as_ref().and_then(|p| p.parse::<u16>().ok()) {
                                ports.push(PortMapping {
                                    container_port,
                                    host_port: hp,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(ContainerInfo {
            id: resp.id.unwrap_or_default(),
            name,
            state: state_str,
            labels,
            ports,
        })
    }

    async fn attach_container(&self, id: &str) -> Result<AttachStreams> {
        use bollard::container::AttachContainerOptions;
        use futures_util::StreamExt;

        let opts = AttachContainerOptions::<&str> {
            stdin: Some(true),
            stdout: Some(true),
            stderr: Some(false),
            stream: Some(true),
            ..Default::default()
        };

        let resp = self.inner.attach_container(id, Some(opts)).await?;

        let output_stream = resp.output.filter_map(|item| async move {
            match item {
                Ok(bollard::container::LogOutput::StdOut { message }) => Some(Ok(message)),
                Ok(bollard::container::LogOutput::StdErr { message }) => Some(Ok(message)),
                Err(e) => Some(Err(std::io::Error::other(e))),
                _ => None,
            }
        });

        let output_reader = tokio_util::io::StreamReader::new(output_stream);

        Ok(AttachStreams {
            output: Box::pin(output_reader),
            input: Box::pin(resp.input),
        })
    }

    async fn list_containers_by_label(&self, label: &str, value: &str) -> Result<Vec<ContainerInfo>> {
        use bollard::container::ListContainersOptions;

        let filter = format!("{label}={value}");
        let opts = ListContainersOptions::<String> {
            all: true,
            filters: HashMap::from([("label".to_string(), vec![filter])]),
            ..Default::default()
        };

        let containers = self.inner.list_containers(Some(opts)).await?;
        let mut result = Vec::new();

        for c in containers {
            let name = c
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_default();
            let state = c.state.unwrap_or_default();
            let labels = c.labels.unwrap_or_default();

            let mut ports = Vec::new();
            if let Some(port_list) = c.ports {
                for p in port_list {
                    if let (Some(priv_port), Some(pub_port)) = (Some(p.private_port), p.public_port) {
                        ports.push(PortMapping {
                            container_port: priv_port,
                            host_port: pub_port,
                        });
                    }
                }
            }

            result.push(ContainerInfo {
                id: c.id.unwrap_or_default(),
                name,
                state,
                labels,
                ports,
            });
        }

        Ok(result)
    }

    async fn wait_container(&self, id: &str) -> Result<i64> {
        use bollard::container::WaitContainerOptions;
        use futures_util::StreamExt;

        let mut stream = self.inner.wait_container(
            id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        );

        match stream.next().await {
            Some(Ok(resp)) => Ok(resp.status_code),
            Some(Err(e)) => Err(e.into()),
            None => Ok(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Container params builder (pure function, testable)
// ---------------------------------------------------------------------------

pub fn container_name(service: &str) -> String {
    format!("mcpfile-{service}")
}

#[must_use]
pub fn build_container_params(
    container_name: &str,
    service_name: &str,
    service: &ServiceConfig,
    env_vars: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
) -> CreateContainerParams {
    let mut labels = HashMap::from([
        ("mcpfile.managed".into(), "true".into()),
        ("mcpfile.service".into(), service_name.into()),
    ]);

    // For debugging: store transport type
    labels.insert(
        "mcpfile.transport".into(),
        match service.transport {
            Transport::Sse => "sse",
            Transport::Stdio => "stdio",
        }
        .into(),
    );

    let mut env: Vec<String> = Vec::new();
    let mut entries: Vec<_> = env_vars.iter().collect();
    entries.sort_by_key(|(k, _)| (*k).clone());
    for (k, v) in &entries {
        env.push(format!("{k}={v}"));
    }

    let mut secret_entries: Vec<_> = secrets.iter().collect();
    secret_entries.sort_by_key(|(k, _)| (*k).clone());
    for (k, v) in &secret_entries {
        env.push(format!("{k}={v}"));
    }

    let exposed_ports = match (&service.transport, service.container_port) {
        (Transport::Sse, Some(port)) => vec![port],
        _ => vec![],
    };

    CreateContainerParams {
        name: container_name.into(),
        image: service.image.clone(),
        labels,
        env,
        exposed_ports,
        stdin_open: matches!(service.transport, Transport::Stdio),
        auto_remove: true,
        command: service.command.clone(),
    }
}

// ---------------------------------------------------------------------------
// Orchestration functions
// ---------------------------------------------------------------------------

pub async fn up_sse(
    docker: &impl DockerClient,
    service_name: &str,
    service: &ServiceConfig,
    env_vars: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
    force: bool,
) -> Result<()> {
    let name = container_name(service_name);

    // Check if already running
    let existing = docker
        .list_containers_by_label("mcpfile.service", service_name)
        .await?;
    let running = existing.iter().any(|c| c.state == "running");

    if running {
        if !force {
            println!("{service_name} is already running");
            return Ok(());
        }
        for c in &existing {
            let _ = docker.stop_container(&c.id).await;
            let _ = docker.remove_container(&c.id, true).await;
        }
    }

    let _ = docker.remove_container(&name, true).await;

    let params = build_container_params(&name, service_name, service, env_vars, secrets);
    let id = docker.create_container(&params).await?;
    docker.start_container(&id).await?;

    let info = docker.inspect_container(&id).await?;
    let url = info
        .ports
        .first()
        .map(|p| format!("http://localhost:{}", p.host_port));

    match url {
        Some(u) => println!("{service_name} is running on {u}"),
        None => println!("{service_name} is running (could not determine port)"),
    }

    Ok(())
}

pub async fn up_foreground(
    docker: &impl DockerClient,
    service_name: &str,
    service: &ServiceConfig,
    env_vars: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
) -> Result<()> {
    let name = container_name(service_name);
    let _ = docker.remove_container(&name, true).await;

    let params = build_container_params(&name, service_name, service, env_vars, secrets);
    let id = docker.create_container(&params).await?;
    docker.start_container(&id).await?;

    let streams = docker.attach_container(&id).await?;
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let (mut up_read, mut up_write) = (streams.output, streams.input);

    tokio::select! {
        r = tokio::io::copy(&mut stdin, &mut up_write) => { r?; }
        r = tokio::io::copy(&mut up_read, &mut stdout) => { r?; }
    }

    let exit_code = docker.wait_container(&id).await?;
    if exit_code != 0 {
        bail!("container exited with code {exit_code}");
    }

    Ok(())
}

pub async fn down(docker: &impl DockerClient, service_name: &str) -> Result<()> {
    let containers = docker
        .list_containers_by_label("mcpfile.service", service_name)
        .await?;

    if containers.is_empty() {
        println!("{service_name} is not running");
        return Ok(());
    }

    for c in &containers {
        let _ = docker.stop_container(&c.id).await;
    }

    println!(
        "{service_name} stopped ({} instance{})",
        containers.len(),
        if containers.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

pub async fn status(docker: &impl DockerClient, config: &Config) -> Result<()> {
    let containers = docker
        .list_containers_by_label("mcpfile.managed", "true")
        .await?;

    let mut container_states: HashMap<String, (String, String)> = HashMap::new();

    for c in &containers {
        let service = c
            .labels
            .get("mcpfile.service")
            .cloned()
            .unwrap_or_else(|| c.name.clone());

        let endpoint = c
            .ports
            .first()
            .map(|p| format!("http://localhost:{}", p.host_port))
            .unwrap_or_else(|| "-".into());

        container_states.insert(service, (c.state.clone(), endpoint));
    }

    println!("{:<15} {:<10} ENDPOINT", "SERVICE", "STATUS");

    let mut names: Vec<&String> = config.services.keys().collect();
    names.sort();

    for name in names {
        let (state, endpoint) = container_states
            .get(name.as_str())
            .map(|(s, p)| (s.as_str(), p.as_str()))
            .unwrap_or(("stopped", "-"));
        println!("{:<15} {:<10} {}", name, state, endpoint);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Docker-based container spawning for bridge
// ---------------------------------------------------------------------------

pub async fn spawn_docker_stdio(
    docker: &impl DockerClient,
    container_name: &str,
    service_name: &str,
    service: &ServiceConfig,
    env_vars: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
) -> Result<(String, AttachStreams)> {
    let params = build_container_params(container_name, service_name, service, env_vars, secrets);
    let id = docker.create_container(&params).await?;
    docker.start_container(&id).await?;
    let streams = docker.attach_container(&id).await?;
    Ok((id, streams))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Clone)]
    pub enum DockerCall {
        Create(String),    // container name
        Start(String),     // id
        Stop(String),      // id
        Remove(String),    // id
        Inspect(String),   // id
        Attach(String),    // id
        List(String, String), // label, value
        Wait(String),      // id
    }

    pub struct MockDockerClient {
        calls: RefCell<Vec<DockerCall>>,
        create_responses: RefCell<Vec<Result<String>>>,
        inspect_responses: RefCell<Vec<Result<ContainerInfo>>>,
        list_responses: RefCell<Vec<Result<Vec<ContainerInfo>>>>,
        wait_responses: RefCell<Vec<Result<i64>>>,
    }

    impl MockDockerClient {
        pub fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                create_responses: RefCell::new(Vec::new()),
                inspect_responses: RefCell::new(Vec::new()),
                list_responses: RefCell::new(Vec::new()),
                wait_responses: RefCell::new(Vec::new()),
            }
        }

        pub fn with_create(self, resp: Result<String>) -> Self {
            self.create_responses.borrow_mut().push(resp);
            self
        }

        pub fn with_inspect(self, resp: Result<ContainerInfo>) -> Self {
            self.inspect_responses.borrow_mut().push(resp);
            self
        }

        pub fn with_list(self, resp: Result<Vec<ContainerInfo>>) -> Self {
            self.list_responses.borrow_mut().push(resp);
            self
        }

        #[allow(dead_code)]
        pub fn with_wait(self, resp: Result<i64>) -> Self {
            self.wait_responses.borrow_mut().push(resp);
            self
        }

        pub fn calls(&self) -> Vec<DockerCall> {
            self.calls.borrow().clone()
        }

        fn pop<T>(store: &RefCell<Vec<Result<T>>>, fallback: T) -> Result<T> {
            let mut q = store.borrow_mut();
            if q.is_empty() {
                return Ok(fallback);
            }
            q.remove(0)
        }
    }

    impl DockerClient for MockDockerClient {
        async fn create_container(&self, params: &CreateContainerParams) -> Result<String> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Create(params.name.clone()));
            Self::pop(&self.create_responses, "mock-id".into())
        }

        async fn start_container(&self, id: &str) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Start(id.into()));
            Ok(())
        }

        async fn stop_container(&self, id: &str) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Stop(id.into()));
            Ok(())
        }

        async fn remove_container(&self, id: &str, _force: bool) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Remove(id.into()));
            Ok(())
        }

        async fn inspect_container(&self, id: &str) -> Result<ContainerInfo> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Inspect(id.into()));
            let fallback = ContainerInfo {
                id: id.into(),
                name: String::new(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![],
            };
            Self::pop(&self.inspect_responses, fallback)
        }

        async fn attach_container(&self, id: &str) -> Result<AttachStreams> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Attach(id.into()));
            let (r, w) = tokio::io::duplex(1024);
            Ok(AttachStreams {
                output: Box::pin(r),
                input: Box::pin(w),
            })
        }

        async fn list_containers_by_label(
            &self,
            label: &str,
            value: &str,
        ) -> Result<Vec<ContainerInfo>> {
            self.calls
                .borrow_mut()
                .push(DockerCall::List(label.into(), value.into()));
            Self::pop(&self.list_responses, vec![])
        }

        async fn wait_container(&self, id: &str) -> Result<i64> {
            self.calls
                .borrow_mut()
                .push(DockerCall::Wait(id.into()));
            Self::pop(&self.wait_responses, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::{DockerCall, MockDockerClient};

    fn test_service(transport: Transport) -> ServiceConfig {
        ServiceConfig {
            image: "test/image:latest".to_string(),
            transport,
            container_port: Some(8080),
            env: HashMap::from([("FOO".to_string(), "bar".to_string())]),
            secrets: HashMap::new(),
            aws_profile: None,
            aws_region: None,
            command: Vec::new(),
        }
    }

    #[test]
    fn build_params_sse() {
        let svc = test_service(Transport::Sse);
        let secrets = HashMap::from([("SECRET".into(), "val".into())]);
        let params =
            build_container_params("mcpfile-grafana", "grafana", &svc, &svc.env, &secrets);

        assert_eq!(params.name, "mcpfile-grafana");
        assert_eq!(params.image, "test/image:latest");
        assert!(!params.stdin_open);
        assert!(params.auto_remove);
        assert_eq!(params.exposed_ports, vec![8080]);
        assert!(params.env.contains(&"FOO=bar".to_string()));
        assert!(params.env.contains(&"SECRET=val".to_string()));
        assert_eq!(params.labels.get("mcpfile.service").unwrap(), "grafana");
    }

    #[test]
    fn build_params_stdio_no_port() {
        let svc = ServiceConfig {
            image: "tldv:latest".into(),
            transport: Transport::Stdio,
            container_port: None,
            env: HashMap::new(),
            secrets: HashMap::new(),
            aws_profile: None,
            aws_region: None,
            command: vec!["cat".into()],
        };
        let params = build_container_params("mcpfile-tldv-abc", "tldv", &svc, &svc.env, &HashMap::new());

        assert!(params.stdin_open);
        assert!(params.exposed_ports.is_empty());
        assert_eq!(params.command, vec!["cat"]);
    }

    #[test]
    fn build_params_env_sorted() {
        let svc = test_service(Transport::Sse);
        let env = HashMap::from([
            ("Z_VAR".into(), "z".into()),
            ("A_VAR".into(), "a".into()),
        ]);
        let params = build_container_params("test", "test", &svc, &env, &HashMap::new());

        let env_strs: Vec<&str> = params.env.iter().map(String::as_str).collect();
        let a_pos = env_strs.iter().position(|e| *e == "A_VAR=a").unwrap();
        let z_pos = env_strs.iter().position(|e| *e == "Z_VAR=z").unwrap();
        assert!(a_pos < z_pos);
    }

    #[tokio::test]
    async fn up_sse_creates_and_starts() {
        let mock = MockDockerClient::new()
            .with_list(Ok(vec![]))
            .with_create(Ok("abc123".into()))
            .with_inspect(Ok(ContainerInfo {
                id: "abc123".into(),
                name: "mcpfile-grafana".into(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![PortMapping {
                    container_port: 8080,
                    host_port: 54321,
                }],
            }));

        let svc = test_service(Transport::Sse);
        up_sse(&mock, "grafana", &svc, &svc.env, &HashMap::new(), false)
            .await
            .unwrap();

        let calls = mock.calls();
        assert!(matches!(&calls[0], DockerCall::List(..)));
        assert!(matches!(&calls[1], DockerCall::Remove(..)));
        assert!(matches!(&calls[2], DockerCall::Create(..)));
        assert!(matches!(&calls[3], DockerCall::Start(..)));
        assert!(matches!(&calls[4], DockerCall::Inspect(..)));
    }

    #[tokio::test]
    async fn up_sse_already_running_skips() {
        let mock = MockDockerClient::new().with_list(Ok(vec![ContainerInfo {
            id: "existing".into(),
            name: "mcpfile-test".into(),
            state: "running".into(),
            labels: HashMap::new(),
            ports: vec![],
        }]));

        let svc = test_service(Transport::Sse);
        up_sse(&mock, "test", &svc, &svc.env, &HashMap::new(), false)
            .await
            .unwrap();

        assert_eq!(mock.calls().len(), 1); // only the list call
    }

    #[tokio::test]
    async fn up_sse_force_stops_then_recreates() {
        let mock = MockDockerClient::new()
            .with_list(Ok(vec![ContainerInfo {
                id: "old-id".into(),
                name: "mcpfile-test".into(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![],
            }]))
            .with_create(Ok("new-id".into()))
            .with_inspect(Ok(ContainerInfo {
                id: "new-id".into(),
                name: "mcpfile-test".into(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![PortMapping {
                    container_port: 8080,
                    host_port: 9999,
                }],
            }));

        let svc = test_service(Transport::Sse);
        up_sse(&mock, "test", &svc, &svc.env, &HashMap::new(), true)
            .await
            .unwrap();

        let calls = mock.calls();
        assert!(matches!(&calls[1], DockerCall::Stop(id) if id == "old-id"));
        assert!(matches!(&calls[2], DockerCall::Remove(id) if id == "old-id"));
    }

    #[tokio::test]
    async fn down_stops_all_matching() {
        let mock = MockDockerClient::new().with_list(Ok(vec![
            ContainerInfo {
                id: "id1".into(),
                name: "mcpfile-tldv-abc".into(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![],
            },
            ContainerInfo {
                id: "id2".into(),
                name: "mcpfile-tldv-def".into(),
                state: "running".into(),
                labels: HashMap::new(),
                ports: vec![],
            },
        ]));

        down(&mock, "tldv").await.unwrap();

        let stops: Vec<_> = mock
            .calls()
            .into_iter()
            .filter(|c| matches!(c, DockerCall::Stop(..)))
            .collect();
        assert_eq!(stops.len(), 2);
    }

    #[tokio::test]
    async fn down_not_running_is_ok() {
        let mock = MockDockerClient::new().with_list(Ok(vec![]));
        down(&mock, "test").await.unwrap();
    }

    #[tokio::test]
    async fn status_shows_services() {
        let mock = MockDockerClient::new().with_list(Ok(vec![ContainerInfo {
            id: "id1".into(),
            name: "mcpfile-grafana".into(),
            state: "running".into(),
            labels: HashMap::from([("mcpfile.service".into(), "grafana".into())]),
            ports: vec![PortMapping {
                container_port: 8080,
                host_port: 54321,
            }],
        }]));

        let config = Config {
            defaults: crate::config::Defaults {
                aws_region: "us-east-1".into(),
                aws_profile: "dev".into(),
            },
            services: HashMap::from([("grafana".into(), test_service(Transport::Sse))]),
        };

        status(&mock, &config).await.unwrap();
    }
}
