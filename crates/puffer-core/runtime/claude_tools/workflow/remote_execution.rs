use crate::{state::ActiveRemoteTarget, AppState};
use anyhow::{bail, Context, Result};
use puffer_config::{load_config, save_user_config, ConfigPaths, SshHostConfig};
use puffer_runner_api::ToolRunner;
use puffer_runner_grpc::RemoteToolRunner;
use puffer_secrets::SecretVault;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteExecutionInput {
    action: String,
    #[serde(default)]
    target_type: Option<String>,
    #[serde(default)]
    host_id: Option<String>,
    #[serde(default)]
    sandbox_id: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    snapshot_id: Option<String>,
    #[serde(default)]
    cpu_millis: Option<u32>,
    #[serde(default)]
    memory_mb: Option<u32>,
    #[serde(default)]
    gpu_count: Option<u32>,
    #[serde(default)]
    gpu_type: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    max_lifetime_seconds: Option<u32>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    port_type: Option<String>,
    #[serde(default)]
    with_memory: Option<bool>,
    #[serde(default)]
    runner_endpoint: Option<String>,
    #[serde(default)]
    runner_host: Option<String>,
    #[serde(default)]
    runner_auth_token: Option<String>,
    #[serde(default)]
    runner_auth_secret_id: Option<String>,
    #[serde(default)]
    runner_cwd: Option<String>,
    #[serde(default)]
    wait_for_ready: Option<bool>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    ssh: Option<SshHostInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshHostInput {
    id: String,
    label: String,
    target: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    auth_secret_id: Option<String>,
}

pub fn execute_remote_execution(state: &mut AppState, cwd: &Path, input: Value) -> Result<String> {
    let parsed: RemoteExecutionInput =
        serde_json::from_value(input).context("invalid RemoteExecution input")?;
    let output = match parsed.action.as_str() {
        "status" | "list-targets" => status(state),
        "ssh:create-config" => ssh_create_config(state, cwd, parsed)?,
        "ssh:delete-config" => ssh_delete_config(state, cwd, parsed)?,
        "enter-remote" => enter_remote(state, cwd, parsed)?,
        "exit-remote" => exit_remote(state),
        "agentenv:list-sandboxes" => {
            agentenv_account(state)?;
            let params = normalize(parsed.workspace)
                .map(|workspace| vec![("workspaceId".to_string(), workspace)])
                .unwrap_or_default();
            agentenv_get_with_params(state, cwd, "/v1/sandboxes/", params)?
        }
        "agentenv:list-snapshots" => {
            agentenv_account(state)?;
            let mut params = Vec::new();
            if let Some(workspace) = normalize(parsed.workspace) {
                params.push(("workspaceId".to_string(), workspace));
            }
            if let Some(sandbox_id) = normalize(parsed.sandbox_id) {
                params.push(("sandboxId".to_string(), sandbox_id));
            }
            agentenv_get_with_params(state, cwd, "/v1/snapshots/", params)?
        }
        "agentenv:get-snapshot" => {
            let snapshot_id = required(parsed.snapshot_id, "snapshotId")?;
            agentenv_get_with_params(state, cwd, &format!("/v1/snapshots/{snapshot_id}"), Vec::new())?
        }
        "agentenv:create-sandbox" => agentenv_create_sandbox(state, cwd, parsed)?,
        "agentenv:terminate-sandbox" | "agentenv:delete-sandbox" => {
            let sandbox_id = required(parsed.sandbox_id, "sandboxId")?;
            agentenv_delete(state, cwd, &format!("/v1/sandboxes/{sandbox_id}"))?
        }
        "agentenv:snapshot-sandbox" => {
            let sandbox_id = required(parsed.sandbox_id, "sandboxId")?;
            let mut body = json!({
                "withMemory": parsed.with_memory.unwrap_or(false),
            });
            if let Some(name) = normalize(parsed.name) {
                body["name"] = json!(name);
            }
            if let Some(description) = normalize(parsed.description) {
                body["description"] = json!(description);
            }
            agentenv_post_with_timeout(
                state,
                cwd,
                &format!("/v1/sandboxes/{sandbox_id}/snapshots"),
                body,
                Duration::from_secs(300),
            )?
        }
        "agentenv:expose-port" => {
            let sandbox_id = required(parsed.sandbox_id, "sandboxId")?;
            let port = parsed.port.context("RemoteExecution requires port")?;
            let port_type = normalize(parsed.port_type).unwrap_or_else(|| "tcp".to_string());
            agentenv_post(
                state,
                cwd,
                &format!("/v1/sandboxes/{sandbox_id}/ports"),
                json!({
                    "ports": [{
                        "containerPort": port,
                        "protocol": port_type,
                    }]
                }),
            )?
        }
        "agentenv:list-ports" => {
            let sandbox_id = required(parsed.sandbox_id, "sandboxId")?;
            agentenv_get_with_params(
                state,
                cwd,
                &format!("/v1/sandboxes/{sandbox_id}/ports"),
                Vec::new(),
            )?
        }
        other => bail!("unsupported RemoteExecution action `{other}`"),
    };
    Ok(serde_json::to_string_pretty(&output)?)
}

fn status(state: &AppState) -> Value {
    let ssh_hosts = state
        .config
        .remote
        .ssh_hosts
        .iter()
        .map(|host| {
            json!({
                "kind": "ssh",
                "id": host.id,
                "label": host.label,
                "target": host.target,
                "port": host.port,
                "cwd": host.cwd,
                "hasAuthSecret": host.auth_secret_id.is_some(),
            })
        })
        .collect::<Vec<_>>();
    let agentenv = state.config.remote.agentenv.as_ref().map(|account| {
        json!({
            "enabled": account.enabled,
            "apiUrl": account.api_url,
            "runnerHost": account.runner_host,
            "workspace": account.workspace,
            "hasCredential": account.credential_secret_id.is_some(),
            "authMethod": account.auth_method,
            "defaults": {
                "sandboxType": account.defaults.sandbox_type,
                "image": account.defaults.image,
                "region": account.defaults.region,
                "cpuMillis": account.defaults.cpu_millis,
                "memoryMb": account.defaults.memory_mb,
                "gpuCount": account.defaults.gpu_count,
                "gpuType": account.defaults.gpu_type,
                "maxLifetimeSeconds": account.defaults.max_lifetime_seconds,
            }
        })
    });
    json!({
        "activeRemoteTarget": state.active_remote_target,
        "defaultTarget": state.config.remote.default_target,
        "sshHosts": ssh_hosts,
        "agentenv": agentenv,
    })
}

fn ssh_create_config(
    state: &mut AppState,
    cwd: &Path,
    input: RemoteExecutionInput,
) -> Result<Value> {
    let ssh = input.ssh.context("RemoteExecution requires ssh config")?;
    let host = SshHostConfig {
        id: required_string(ssh.id, "ssh.id")?,
        label: required_string(ssh.label, "ssh.label")?,
        target: required_string(ssh.target, "ssh.target")?,
        port: validate_ssh_port(ssh.port)?,
        cwd: normalize(ssh.cwd),
        auth_secret_id: normalize(ssh.auth_secret_id),
    };
    state
        .config
        .remote
        .ssh_hosts
        .retain(|item| item.id != host.id);
    state.config.remote.ssh_hosts.push(host.clone());
    save_current_config(cwd, state)?;
    Ok(json!({
        "ok": true,
        "action": "ssh:create-config",
        "host": {
            "id": host.id,
            "label": host.label,
            "target": host.target,
            "port": host.port,
            "cwd": host.cwd,
            "hasAuthSecret": host.auth_secret_id.is_some(),
        }
    }))
}

fn ssh_delete_config(
    state: &mut AppState,
    cwd: &Path,
    input: RemoteExecutionInput,
) -> Result<Value> {
    let host_id = required(input.host_id, "hostId")?;
    let before = state.config.remote.ssh_hosts.len();
    state
        .config
        .remote
        .ssh_hosts
        .retain(|host| host.id != host_id);
    if state.config.remote.default_target.as_deref() == Some(&format!("ssh:{host_id}")) {
        state.config.remote.default_target = None;
    }
    if state
        .active_remote_target
        .as_ref()
        .is_some_and(|target| target.kind == "ssh" && target.id == host_id)
    {
        state.active_remote_target = None;
    }
    save_current_config(cwd, state)?;
    Ok(json!({
        "ok": true,
        "action": "ssh:delete-config",
        "deleted": state.config.remote.ssh_hosts.len() != before,
        "hostId": host_id,
    }))
}

fn enter_remote(state: &mut AppState, cwd: &Path, input: RemoteExecutionInput) -> Result<Value> {
    let target_type = required(input.target_type, "targetType")?;
    let target_type = normalize_target_type(&target_type)?;
    let mut configured_cwd = None;
    let mut ssh_host = None;
    let id = match target_type.as_str() {
        "ssh" => {
            let host_id = required(input.host_id, "hostId")?;
            let host = find_ssh_host(state, cwd, &host_id)?;
            let Some(host) = host else {
                bail!("unknown SSH host `{host_id}`");
            };
            configured_cwd = host.cwd.clone();
            ssh_host = Some(host);
            host_id
        }
        "agentenv" => required(input.sandbox_id.or(input.host_id), "sandboxId")?,
        other => bail!("unsupported remote target type `{other}`"),
    };
    let mut runner_auth_token = if let Some(token) = normalize(input.runner_auth_token) {
        Some(token)
    } else if let Some(token) = reveal_optional_secret(cwd, input.runner_auth_secret_id.as_deref())?
    {
        Some(token)
    } else {
        state
            .config
            .remote_runner
            .as_ref()
            .and_then(|runner| runner.resolve_auth_token())
    };
    let mut bootstrapped_ssh = false;
    let mut exposed_agentenv_port = None;
    let mut ssh_bootstrap_child = None;
    let runner_endpoint = if let Some(endpoint) = normalize(input.runner_endpoint) {
        endpoint
    } else if let Some(endpoint) = state
        .config
        .remote_runner
        .as_ref()
        .map(|runner| runner.endpoint.clone())
    {
        endpoint
    } else if let Some(host) = ssh_host.as_ref() {
        let runner_cwd = normalize(input.runner_cwd.clone())
            .or_else(|| configured_cwd.clone())
            .unwrap_or_else(|| "~".to_string());
        runner_auth_token = None;
        let bootstrap = bootstrapped_ssh_runner(host, None, &runner_cwd)?;
        bootstrapped_ssh = true;
        ssh_bootstrap_child = bootstrap.child;
        bootstrap.endpoint
    } else if target_type == "agentenv" {
        let runner_port = input.port.unwrap_or(50051);
        let runner_host = normalize(input.runner_host.clone()).or_else(|| {
            state
                .config
                .remote
                .agentenv
                .as_ref()
                .and_then(|account| account.runner_host.clone())
        });
        let discovered = discover_agentenv_runner_endpoint(
            state,
            cwd,
            &id,
            runner_port,
            runner_host.as_deref(),
        )?;
        exposed_agentenv_port = discovered.exposed_port;
        discovered.endpoint
    } else {
        bail!("RemoteExecution enter-remote requires runnerEndpoint or a configured remote target");
    };
    let runner = RemoteToolRunner::connect(&runner_endpoint, runner_auth_token.as_deref())
        .map_err(|error| anyhow::anyhow!("connect remote tool runner: {error}"))?;
    if input.wait_for_ready.unwrap_or(true) {
        if let Err(error) = wait_for_runner_ready(&runner, Duration::from_secs(20)) {
            if let Some(mut child) = ssh_bootstrap_child.take() {
                let ssh_detail = terminate_ssh_bootstrap(&mut child);
                if ssh_detail.is_empty() {
                    return Err(error);
                }
                bail!("{error}; ssh bootstrap detail: {ssh_detail}");
            }
            return Err(error);
        }
    }
    if state.local_tool_runner.is_none() {
        state.local_tool_runner = Some(state.tool_runner.clone());
    }
    state.tool_runner = Arc::new(runner);
    let active_cwd = normalize(input.runner_cwd)
        .or(configured_cwd)
        .or_else(|| (target_type == "agentenv").then(|| "/user".to_string()))
        .map(PathBuf::from);
    state.remote_name = Some(format!("{target_type}:{id}"));
    state.remote_environment = Some(target_type.clone());
    state.remote_session_id = Some(id.clone());
    state.remote_session_url = Some(runner_endpoint.clone());
    state.remote_session_status = Some("connected".to_string());
    state.active_remote_runner_auth_token = runner_auth_token.clone();
    state.active_remote_target = Some(ActiveRemoteTarget {
        kind: target_type,
        id,
    });
    state.active_remote_cwd = active_cwd.clone();
    Ok(json!({
        "ok": true,
        "action": "enter-remote",
        "activeRemoteTarget": state.active_remote_target,
        "runnerEndpoint": runner_endpoint,
        "runnerCwd": active_cwd,
        "bootstrappedSsh": bootstrapped_ssh,
        "exposedAgentEnvPort": exposed_agentenv_port,
        "note": "Remote target connected. Runner-supported tools now execute through the remote tool runner.",
    }))
}

fn exit_remote(state: &mut AppState) -> Value {
    if let Some(local_runner) = state.local_tool_runner.clone() {
        state.tool_runner = local_runner;
    }
    state.remote_name = None;
    state.remote_environment = None;
    state.remote_session_id = None;
    state.remote_session_url = None;
    state.remote_session_status = None;
    state.active_remote_runner_auth_token = None;
    state.active_remote_target = None;
    state.active_remote_cwd = None;
    json!({
        "ok": true,
        "action": "exit-remote",
        "activeRemoteTarget": null,
    })
}

struct SshRunnerBootstrap {
    endpoint: String,
    child: Option<Child>,
}

fn bootstrapped_ssh_runner(
    host: &SshHostConfig,
    runner_auth_token: Option<&str>,
    runner_cwd: &str,
) -> Result<SshRunnerBootstrap> {
    validate_ssh_port(host.port)?;
    let local_port = allocate_local_port().context("allocate local SSH tunnel port")?;
    let remote_port = local_port;
    let remote_command = ssh_runner_remote_command(remote_port, runner_auth_token, runner_cwd);
    let mut command = Command::new("ssh");
    command
        .arg("-o")
        .arg("BatchMode=no")
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ConnectTimeout=15")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-L")
        .arg(format!("{local_port}:127.0.0.1:{remote_port}"));
    if let Some(port) = host.port {
        command.arg("-p").arg(port.to_string());
    }
    command
        .arg(&host.target)
        .arg(remote_command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .with_context(|| format!("spawn SSH runner tunnel to {}", host.target))?;
    Ok(SshRunnerBootstrap {
        endpoint: format!("http://127.0.0.1:{local_port}"),
        child: Some(child),
    })
}

fn terminate_ssh_bootstrap(child: &mut Child) -> String {
    let mut stderr = child.stderr.take();
    let _ = child.kill();
    let _ = child.wait();
    let mut detail = String::new();
    if let Some(mut stderr) = stderr.take() {
        let _ = stderr.read_to_string(&mut detail);
    }
    detail.trim().to_string()
}

fn allocate_local_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn ssh_runner_remote_command(
    port: u16,
    runner_auth_token: Option<&str>,
    runner_cwd: &str,
) -> String {
    if looks_like_windows_path(runner_cwd) {
        return windows_ssh_runner_remote_command(port, runner_auth_token, runner_cwd);
    }

    let args = format!(
        "--bind {} --cwd {}",
        shell_quote(&format!("127.0.0.1:{port}")),
        shell_quote(runner_cwd)
    );
    match runner_auth_token {
        Some(token) => format!(
            "PUFFER_RUNNER_TOKEN={} exec puffer-tool-runner {}",
            shell_quote(token),
            args
        ),
        None => format!("exec puffer-tool-runner {args} --allow-unauthenticated"),
    }
}

fn windows_ssh_runner_remote_command(
    port: u16,
    runner_auth_token: Option<&str>,
    runner_cwd: &str,
) -> String {
    let mut args = format!(
        "--bind {} --cwd {}",
        cmd_quote(&format!("127.0.0.1:{port}")),
        cmd_quote(runner_cwd)
    );
    if runner_auth_token.is_none() {
        args.push_str(" --allow-unauthenticated");
    }
    let runner = format!(
        "(puffer-tool-runner.exe {args} || {} {args})",
        cmd_quote("%USERPROFILE%\\.cargo\\bin\\puffer-tool-runner.exe")
    );
    match runner_auth_token {
        Some(token) => format!("set PUFFER_RUNNER_TOKEN={}&& {runner}", cmd_quote(token)),
        None => runner,
    }
}

fn looks_like_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
}

fn wait_for_runner_ready(runner: &RemoteToolRunner, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    let mut last_error = None;
    while started.elapsed() <= timeout {
        match runner.ping() {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
                thread::sleep(Duration::from_millis(250));
            }
        }
    }
    let detail = last_error.unwrap_or_else(|| "runner did not respond".to_string());
    bail!("remote tool runner did not respond to ping: {detail}");
}

pub(crate) fn reconnect_active_remote_runner(state: &mut AppState, cwd: &Path) -> Result<bool> {
    let Some(target) = state.active_remote_target.clone() else {
        return Ok(false);
    };
    let active_cwd = state.active_remote_cwd.clone();
    let active_cwd_string = active_cwd
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    let (endpoint, auth_token, active_cwd) = match target.kind.as_str() {
        "ssh" => {
            let host = find_ssh_host(state, cwd, &target.id)?;
            let Some(host) = host else {
                bail!(
                    "cannot reconnect SSH remote `{}`: host config is missing",
                    target.id
                );
            };
            let runner_cwd = active_cwd_string
                .or_else(|| host.cwd.clone())
                .unwrap_or_else(|| "~".to_string());
            let bootstrap = bootstrapped_ssh_runner(&host, None, &runner_cwd)
                .with_context(|| format!("re-bootstrap SSH remote `{}`", target.id))?;
            (bootstrap.endpoint, None, Some(PathBuf::from(runner_cwd)))
        }
        "agentenv" => {
            let runner_host = state
                .config
                .remote
                .agentenv
                .as_ref()
                .and_then(|account| account.runner_host.clone());
            let discovered = discover_agentenv_runner_endpoint(
                state,
                cwd,
                &target.id,
                50051,
                runner_host.as_deref(),
            );
            let endpoint = match discovered {
                Ok(discovered) => discovered.endpoint,
                Err(error) => state.remote_session_url.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot reconnect AgentEnv remote `{}`: endpoint discovery failed: {error}",
                        target.id
                    )
                })?,
            };
            (
                endpoint,
                state.active_remote_runner_auth_token.clone(),
                active_cwd.or_else(|| Some(PathBuf::from("/user"))),
            )
        }
        _ => {
            let endpoint = state.remote_session_url.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot reconnect remote `{}`: no runner endpoint is saved",
                    target.id
                )
            })?;
            (
                endpoint,
                state.active_remote_runner_auth_token.clone(),
                active_cwd,
            )
        }
    };

    let runner = RemoteToolRunner::connect(&endpoint, auth_token.as_deref())
        .map_err(|error| anyhow::anyhow!("reconnect remote tool runner: {error}"))?;
    wait_for_runner_ready(&runner, Duration::from_secs(20))?;
    if state.local_tool_runner.is_none() {
        state.local_tool_runner = Some(state.tool_runner.clone());
    }
    state.tool_runner = Arc::new(runner);
    state.remote_name = Some(format!("{}:{}", target.kind, target.id));
    state.remote_environment = Some(target.kind.clone());
    state.remote_session_id = Some(target.id.clone());
    state.remote_session_url = Some(endpoint);
    state.remote_session_status = Some("connected".to_string());
    state.active_remote_runner_auth_token = auth_token;
    state.active_remote_target = Some(target);
    state.active_remote_cwd = active_cwd;
    Ok(true)
}

pub(crate) fn remote_error_is_reconnectable(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_lowercase();
    message.contains("transport error")
        || message.contains("connection error")
        || message.contains("connection refused")
        || message.contains("connection reset")
        || message.contains("broken pipe")
        || message.contains("status: unavailable")
        || message.contains("code: unavailable")
        || message.contains("operation timed out")
        || message.contains("deadline has elapsed")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn cmd_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn agentenv_create_sandbox(
    state: &AppState,
    cwd: &Path,
    input: RemoteExecutionInput,
) -> Result<Value> {
    let account = agentenv_account(state)?;
    let workspace = normalize(input.workspace.clone())
        .or_else(|| account.workspace.clone())
        .context("AgentEnv workspace is required")?;
    validate_agentenv_workspace_id(&workspace)?;
    let defaults = &account.defaults;
    let (default_cpu_millis, default_memory_mb) =
        agentenv_sandbox_type_resources(&defaults.sandbox_type);
    let env = agentenv_create_env(&input);
    let mut body = json!({
        "workspaceId": workspace,
        "image": agentenv_image_for_create(normalize(input.image.clone()).unwrap_or_else(|| defaults.image.clone())),
        "cpuMillis": positive_u32(input.cpu_millis)
            .or_else(|| positive_u32(defaults.cpu_millis))
            .unwrap_or(default_cpu_millis),
        "memoryMb": positive_u32(input.memory_mb)
            .or_else(|| positive_u32(defaults.memory_mb))
            .unwrap_or(default_memory_mb),
        "gpuCount": input.gpu_count.or(defaults.gpu_count).unwrap_or(0),
        "env": env,
        "args": [],
    });
    set_optional(&mut body, "name", normalize(input.name));
    set_optional(&mut body, "description", normalize(input.description));
    set_optional(&mut body, "snapshotId", normalize(input.snapshot_id));
    set_optional(
        &mut body,
        "gpuType",
        normalize(input.gpu_type).or_else(|| defaults.gpu_type.clone()),
    );
    set_optional_u32(
        &mut body,
        "maxLifetimeSeconds",
        positive_u32(input.max_lifetime_seconds)
            .or_else(|| positive_u32(defaults.max_lifetime_seconds)),
    );
    if let Some(region) = normalize(input.region).or_else(|| defaults.region.clone()) {
        body["allowedRegions"] = json!([region]);
    }
    agentenv_post(state, cwd, "/v1/sandboxes/", body)
}

fn agentenv_create_env(input: &RemoteExecutionInput) -> BTreeMap<String, String> {
    let mut env = input
        .env
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(token) = normalize(input.runner_auth_token.clone()) {
        env.entry("MANAGED_AGENT_PUFFER_RUNNER_TOKEN".to_string())
            .or_insert(token);
    }
    env
}

fn agentenv_sandbox_type_resources(sandbox_type: &str) -> (u32, u32) {
    match sandbox_type.trim().to_ascii_lowercase().as_str() {
        "micro" => (1000, 1024),
        "medium" => (4000, 8192),
        "large" => (8000, 16384),
        "xl" | "xlarge" => (16000, 32768),
        _ => (2000, 4096),
    }
}

fn agentenv_image_for_create(image: String) -> String {
    let trimmed = image.trim();
    if let Some(rest) = trimmed.strip_prefix("docker.io/library/") {
        format!("docker://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("registry-1.docker.io/library/") {
        format!("docker://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("docker.io/") {
        format!("docker://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("registry-1.docker.io/") {
        format!("docker://{rest}")
    } else if trimmed.starts_with("docker://") {
        trimmed.to_string()
    } else {
        format!("docker://{trimmed}")
    }
}

struct AgentEnvRunnerDiscovery {
    endpoint: String,
    exposed_port: Option<u16>,
}

fn discover_agentenv_runner_endpoint(
    state: &AppState,
    cwd: &Path,
    sandbox_id: &str,
    runner_port: u16,
    runner_host: Option<&str>,
) -> Result<AgentEnvRunnerDiscovery> {
    if runner_port == 0 {
        bail!("AgentEnv runner port must be greater than 0");
    }
    let sandbox = agentenv_get_with_params(
        state,
        cwd,
        &format!("/v1/sandboxes/{sandbox_id}"),
        Vec::new(),
    )?;
    if let Some(endpoint) = extract_agentenv_runner_endpoint(&sandbox, Some(runner_port)) {
        return Ok(AgentEnvRunnerDiscovery {
            endpoint,
            exposed_port: None,
        });
    }
    if let Some(endpoint) =
        extract_agentenv_runner_endpoint_with_host(&sandbox, Some(runner_port), runner_host)
    {
        return Ok(AgentEnvRunnerDiscovery {
            endpoint,
            exposed_port: None,
        });
    }

    if let Ok(ports) = agentenv_get_with_params(
        state,
        cwd,
        &format!("/v1/sandboxes/{sandbox_id}/ports"),
        Vec::new(),
    ) {
        if let Some(endpoint) = extract_agentenv_runner_endpoint(&ports, Some(runner_port)) {
            return Ok(AgentEnvRunnerDiscovery {
                endpoint,
                exposed_port: None,
            });
        }
        if let Some(endpoint) =
            extract_agentenv_runner_endpoint_with_host(&ports, Some(runner_port), runner_host)
        {
            return Ok(AgentEnvRunnerDiscovery {
                endpoint,
                exposed_port: None,
            });
        }
    }

    let exposed = agentenv_post(
        state,
        cwd,
        &format!("/v1/sandboxes/{sandbox_id}/ports"),
        json!({
            "ports": [{
                "containerPort": runner_port,
                "protocol": "tcp",
            }]
        }),
    )?;
    if let Some(endpoint) = extract_agentenv_runner_endpoint(&exposed, Some(runner_port)) {
        return Ok(AgentEnvRunnerDiscovery {
            endpoint,
            exposed_port: Some(runner_port),
        });
    }
    if let Some(endpoint) =
        extract_agentenv_runner_endpoint_with_host(&exposed, Some(runner_port), runner_host)
    {
        return Ok(AgentEnvRunnerDiscovery {
            endpoint,
            exposed_port: Some(runner_port),
        });
    }

    if runner_host.is_some() {
        bail!(
            "could not discover AgentEnv runner endpoint for sandbox `{sandbox_id}` on port {runner_port}; AgentEnv did not return a matching host port"
        )
    } else {
        bail!(
            "could not discover AgentEnv runner endpoint for sandbox `{sandbox_id}` on port {runner_port}; pass runnerEndpoint explicitly or configure AgentEnv runnerHost"
        )
    }
}

fn agentenv_get_with_params(
    state: &AppState,
    cwd: &Path,
    path: &str,
    params: Vec<(String, String)>,
) -> Result<Value> {
    let (url, auth) = agentenv_request_context(state, cwd, path)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("build AgentEnv HTTP client")?;
    let params_json =
        serde_json::to_string(&params).unwrap_or_else(|_| "<unserializable>".to_string());
    let response = apply_agentenv_auth(client.get(&url).query(&params), &auth)
        .send()
        .map_err(|error| {
            anyhow::anyhow!(
                "send AgentEnv GET request to {path} ({url}) with params {params_json}: {}",
                format_error_chain(&error)
            )
        })?;
    agentenv_response_json(response)
}

fn agentenv_post(state: &AppState, cwd: &Path, path: &str, body: Value) -> Result<Value> {
    agentenv_post_with_timeout(state, cwd, path, body, Duration::from_secs(60))
}

fn agentenv_post_with_timeout(
    state: &AppState,
    cwd: &Path,
    path: &str,
    body: Value,
    timeout: Duration,
) -> Result<Value> {
    let (url, auth) = agentenv_request_context(state, cwd, path)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .context("build AgentEnv HTTP client")?;
    let body_json = serde_json::to_string(&body).unwrap_or_else(|_| "<unserializable>".to_string());
    let response = apply_agentenv_auth(client.post(&url).json(&body), &auth)
        .send()
        .map_err(|error| {
            anyhow::anyhow!(
                "send AgentEnv POST request to {path} ({url}) with body {body_json}: {}",
                format_error_chain(&error)
            )
        })?;
    agentenv_response_json_with_context(
        response,
        Some(json!({
            "path": path,
            "requestBody": body,
        })),
    )
}

fn agentenv_delete(state: &AppState, cwd: &Path, path: &str) -> Result<Value> {
    let (url, auth) = agentenv_request_context(state, cwd, path)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("build AgentEnv HTTP client")?;
    let response = apply_agentenv_auth(client.delete(&url), &auth)
        .send()
        .map_err(|error| {
            anyhow::anyhow!(
                "send AgentEnv DELETE request to {path} ({url}): {}",
                format_error_chain(&error)
            )
        })?;
    if response.status().is_success() {
        Ok(json!({ "ok": true }))
    } else {
        agentenv_response_json(response)
    }
}

enum AgentEnvAuth {
    ApiKey(String),
    AccessToken(String),
}

fn agentenv_request_context(
    state: &AppState,
    cwd: &Path,
    path: &str,
) -> Result<(String, AgentEnvAuth)> {
    let account = agentenv_account(state)?;
    let credential = reveal_agentenv_credential(cwd, account.credential_secret_id.as_deref())?;
    let auth = match account.auth_method.as_str() {
        "access_token" => AgentEnvAuth::AccessToken(credential),
        _ => AgentEnvAuth::ApiKey(credential),
    };
    Ok((agentenv_url(&account.api_url, path), auth))
}

fn agentenv_account(state: &AppState) -> Result<&puffer_config::AgentEnvAccountConfig> {
    let account = state
        .config
        .remote
        .agentenv
        .as_ref()
        .context("AgentEnv account is not configured")?;
    if !account.enabled {
        bail!("AgentEnv account is disabled");
    }
    Ok(account)
}

fn reveal_agentenv_credential(cwd: &Path, secret_id: Option<&str>) -> Result<String> {
    let secret_id = secret_id.context("AgentEnv credential secret is not configured")?;
    let paths = ConfigPaths::discover(cwd);
    let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
        .context("open Puffer secret vault")?;
    let secret = vault
        .reveal(secret_id)
        .with_context(|| format!("reveal AgentEnv credential secret `{secret_id}`"))?;
    Ok(secret.value)
}

fn reveal_optional_secret(cwd: &Path, secret_id: Option<&str>) -> Result<Option<String>> {
    let Some(secret_id) = secret_id.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }) else {
        return Ok(None);
    };
    let paths = ConfigPaths::discover(cwd);
    let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))
        .context("open Puffer secret vault")?;
    let secret = vault
        .reveal(secret_id)
        .with_context(|| format!("reveal runner auth secret `{secret_id}`"))?;
    Ok(Some(secret.value))
}

fn apply_agentenv_auth(
    request: reqwest::blocking::RequestBuilder,
    auth: &AgentEnvAuth,
) -> reqwest::blocking::RequestBuilder {
    match auth {
        AgentEnvAuth::ApiKey(value) => request.header("X-API-Key", value),
        AgentEnvAuth::AccessToken(value) => request.bearer_auth(value),
    }
}

fn agentenv_response_json(response: reqwest::blocking::Response) -> Result<Value> {
    agentenv_response_json_with_context(response, None)
}

fn agentenv_response_json_with_context(
    response: reqwest::blocking::Response,
    context: Option<Value>,
) -> Result<Value> {
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "text": text }));
    if status.is_success() {
        Ok(parsed)
    } else {
        let mut output = json!({
            "ok": false,
            "status": status.as_u16(),
            "error": parsed,
        });
        if let Some(context) = context {
            output["request"] = context;
        }
        Ok(output)
    }
}

fn agentenv_url(api_url: &str, path: &str) -> String {
    let base = api_url.trim().trim_end_matches('/').trim_end_matches("/v1");
    let normalized = if path.starts_with("/v1/") {
        path.to_string()
    } else if path.starts_with('/') {
        format!("/v1{path}")
    } else {
        format!("/v1/{path}")
    };
    format!("{base}{normalized}")
}

fn format_error_chain(error: &(dyn Error + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(error) = source {
        parts.push(error.to_string());
        source = error.source();
    }
    parts.join(": ")
}

fn extract_agentenv_runner_endpoint(value: &Value, preferred_port: Option<u16>) -> Option<String> {
    let mut urls = Vec::new();
    collect_agentenv_endpoint_candidates(value, preferred_port, &mut urls);
    urls.into_iter().next()
}

fn extract_agentenv_runner_endpoint_with_host(
    value: &Value,
    preferred_port: Option<u16>,
    runner_host: Option<&str>,
) -> Option<String> {
    let runner_host = runner_host.map(str::trim).filter(|host| !host.is_empty())?;
    let mut ports = Vec::new();
    collect_agentenv_host_ports(value, preferred_port, &mut ports);
    ports
        .into_iter()
        .next()
        .map(|port| format!("http://{runner_host}:{port}"))
}

fn collect_agentenv_host_ports(value: &Value, preferred_port: Option<u16>, ports: &mut Vec<u16>) {
    match value {
        Value::Object(map) => {
            if object_matches_preferred_port(map, preferred_port) {
                if let Some(port) = numeric_field(map, &["publicPort", "externalPort", "hostPort"])
                {
                    ports.push(port);
                }
            }
            for child in map.values() {
                collect_agentenv_host_ports(child, preferred_port, ports);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_agentenv_host_ports(item, preferred_port, ports);
            }
        }
        _ => {}
    }
}

fn collect_agentenv_endpoint_candidates(
    value: &Value,
    preferred_port: Option<u16>,
    urls: &mut Vec<String>,
) {
    match value {
        Value::Object(map) => {
            if let Some(endpoint) = endpoint_from_port_object(map, preferred_port) {
                urls.push(endpoint);
            }
            let object_matches_preferred_port = object_matches_preferred_port(map, preferred_port);

            for (key, child) in map {
                if object_matches_preferred_port && likely_endpoint_key(key) {
                    if let Some(url) = child.as_str().and_then(normalize_endpoint_url) {
                        urls.push(url);
                    }
                }
                collect_agentenv_endpoint_candidates(child, preferred_port, urls);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_agentenv_endpoint_candidates(item, preferred_port, urls);
            }
        }
        _ => {}
    }
}

fn endpoint_from_port_object(
    map: &serde_json::Map<String, Value>,
    preferred_port: Option<u16>,
) -> Option<String> {
    if let Some(port) = preferred_port {
        let object_port = numeric_field(
            map,
            &["containerPort", "targetPort", "port", "internalPort"],
        );
        if object_port.is_some_and(|object_port| object_port != port) {
            return None;
        }
    }

    for key in [
        "runnerEndpoint",
        "toolRunnerEndpoint",
        "grpcEndpoint",
        "endpoint",
        "publicUrl",
        "publicURL",
        "url",
        "host",
        "hostname",
    ] {
        if let Some(url) = map
            .get(key)
            .and_then(Value::as_str)
            .and_then(normalize_endpoint_url)
        {
            return Some(url);
        }
    }

    let host = map
        .get("host")
        .or_else(|| map.get("hostname"))
        .and_then(Value::as_str)?;
    let external_port = numeric_field(map, &["publicPort", "externalPort", "hostPort"]);
    let port = external_port.or(preferred_port)?;
    let scheme = map
        .get("scheme")
        .or_else(|| map.get("protocol"))
        .and_then(Value::as_str)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https"))
        .unwrap_or("http");
    Some(format!("{scheme}://{host}:{port}"))
}

fn object_matches_preferred_port(
    map: &serde_json::Map<String, Value>,
    preferred_port: Option<u16>,
) -> bool {
    let Some(preferred_port) = preferred_port else {
        return true;
    };
    numeric_field(
        map,
        &["containerPort", "targetPort", "port", "internalPort"],
    )
    .is_none_or(|object_port| object_port == preferred_port)
}

fn likely_endpoint_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("runner")
        || normalized.contains("grpc")
        || normalized == "endpoint"
        || normalized == "publicurl"
        || normalized == "url"
}

fn normalize_endpoint_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    None
}

fn numeric_field(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u16> {
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        if let Some(number) = value.as_u64() {
            return u16::try_from(number).ok();
        }
        value.as_str()?.parse::<u16>().ok()
    })
}

fn save_current_config(cwd: &Path, state: &AppState) -> Result<()> {
    let paths = ConfigPaths::discover(cwd);
    save_user_config(&paths, &state.config).context("save Puffer user config")
}

fn find_ssh_host(state: &mut AppState, cwd: &Path, host_id: &str) -> Result<Option<SshHostConfig>> {
    let in_memory_host = state
        .config
        .remote
        .ssh_hosts
        .iter()
        .find(|host| host.id == host_id)
        .cloned();
    let paths = ConfigPaths::discover(cwd);
    let latest = load_config(&paths).context("load latest Puffer config")?;
    let host = latest
        .remote
        .ssh_hosts
        .iter()
        .find(|host| host.id == host_id)
        .cloned();
    state.config.remote = latest.remote;
    if host.is_some() {
        return Ok(host);
    }

    Ok(in_memory_host)
}

fn required(value: Option<String>, field: &str) -> Result<String> {
    normalize(value).with_context(|| format!("RemoteExecution requires {field}"))
}

fn required_string(value: String, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("RemoteExecution requires {field}");
    }
    Ok(trimmed.to_string())
}

fn validate_ssh_port(port: Option<u16>) -> Result<Option<u16>> {
    match port {
        Some(1) => bail!("SSH port 1 is invalid for Puffer remote execution; omit port or use 22"),
        Some(0) => bail!("SSH port must be greater than 0"),
        other => Ok(other),
    }
}

fn normalize(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn validate_agentenv_workspace_id(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && matches!(
            (bytes.get(8), bytes.get(13), bytes.get(18), bytes.get(23)),
            (Some(b'-'), Some(b'-'), Some(b'-'), Some(b'-'))
        )
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit());
    if !valid {
        bail!("AgentEnv workspace must be the full workspace UUID, not a shortened display value");
    }
    Ok(())
}

fn normalize_target_type(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ssh" => Ok("ssh".to_string()),
        "agentenv" | "agent-env" => Ok("agentenv".to_string()),
        other => bail!("unsupported remote target type `{other}`"),
    }
}

fn set_optional(body: &mut Value, key: &str, value: Option<String>) {
    if let Some(value) = value {
        body[key] = json!(value);
    }
}

fn set_optional_u32(body: &mut Value, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        body[key] = json!(value);
    }
}

fn positive_u32(value: Option<u32>) -> Option<u32> {
    value.filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_config::PufferConfig;
    use puffer_session_store::SessionMetadata;
    use std::ffi::OsString;
    use std::sync::Arc;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use uuid::Uuid;

    fn temp_state() -> AppState {
        let tempdir = tempfile::tempdir().unwrap();
        let cwd = tempdir.path().to_path_buf();
        std::mem::forget(tempdir);
        let session = SessionMetadata {
            id: Uuid::new_v4(),
            display_name: None,
            generated_title: None,
            cwd: cwd.clone(),
            created_at_ms: 0,
            updated_at_ms: 0,
            parent_session_id: None,
            slug: None,
            tags: Vec::new(),
            note: None,
        };
        AppState::new(PufferConfig::default(), cwd, session)
    }

    fn puffer_home_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct ScopedPufferHome {
        old_value: Option<OsString>,
    }

    impl ScopedPufferHome {
        fn set(path: &Path) -> Self {
            let old_value = std::env::var_os("PUFFER_HOME");
            std::env::set_var("PUFFER_HOME", path);
            Self { old_value }
        }
    }

    impl Drop for ScopedPufferHome {
        fn drop(&mut self) {
            if let Some(value) = self.old_value.take() {
                std::env::set_var("PUFFER_HOME", value);
            } else {
                std::env::remove_var("PUFFER_HOME");
            }
        }
    }

    #[test]
    fn agentenv_url_normalizes_v1_suffix() {
        assert_eq!(
            agentenv_url("https://api.agentenv.io/v1", "/v1/sandboxes/"),
            "https://api.agentenv.io/v1/sandboxes/"
        );
        assert_eq!(
            agentenv_url("https://api.agentenv.io", "/sandboxes/"),
            "https://api.agentenv.io/v1/sandboxes/"
        );
    }

    #[test]
    fn agentenv_create_defaults_match_cli_shape() {
        assert_eq!(
            agentenv_image_for_create("python:3.11-slim".to_string()),
            "docker://python:3.11-slim"
        );
        assert_eq!(
            agentenv_image_for_create("docker.io/library/python:3.11-slim".to_string()),
            "docker://python:3.11-slim"
        );
        assert_eq!(
            agentenv_image_for_create("docker.io/bitnami/python:3.11".to_string()),
            "docker://bitnami/python:3.11"
        );
        assert_eq!(agentenv_sandbox_type_resources("small"), (2000, 4096));
        assert_eq!(positive_u32(Some(0)), None);
        assert_eq!(positive_u32(Some(900)), Some(900));
    }

    #[test]
    fn agentenv_create_maps_runner_auth_token_to_entrypoint_env() {
        let input = RemoteExecutionInput {
            action: "agentenv:create-sandbox".to_string(),
            target_type: None,
            host_id: None,
            sandbox_id: None,
            workspace: None,
            name: None,
            description: None,
            image: None,
            snapshot_id: None,
            cpu_millis: None,
            memory_mb: None,
            gpu_count: None,
            gpu_type: None,
            region: None,
            max_lifetime_seconds: None,
            port: None,
            port_type: None,
            with_memory: None,
            runner_endpoint: None,
            runner_host: None,
            runner_auth_token: Some("runner-test-token".to_string()),
            runner_auth_secret_id: None,
            runner_cwd: None,
            wait_for_ready: None,
            env: BTreeMap::from([("EXTRA".to_string(), "value".to_string())]),
            ssh: None,
        };

        let env = agentenv_create_env(&input);

        assert_eq!(
            env.get("MANAGED_AGENT_PUFFER_RUNNER_TOKEN")
                .map(String::as_str),
            Some("runner-test-token")
        );
        assert_eq!(env.get("EXTRA").map(String::as_str), Some("value"));
    }

    #[test]
    fn validates_agentenv_workspace_id_shape() {
        assert!(validate_agentenv_workspace_id("861a219c-19b7-4686-89e1-63fdc12d6ef5").is_ok());
        assert!(validate_agentenv_workspace_id("861a219c-19b").is_err());
        assert!(validate_agentenv_workspace_id("not-a-workspace").is_err());
    }

    #[test]
    fn extracts_agentenv_runner_endpoint_from_sandbox_metadata() {
        let value = json!({
            "id": "sbx_1",
            "metadata": {
                "toolRunnerEndpoint": "https://runner.example.com"
            }
        });

        assert_eq!(
            extract_agentenv_runner_endpoint(&value, Some(50051)).as_deref(),
            Some("https://runner.example.com")
        );
    }

    #[test]
    fn extracts_agentenv_runner_endpoint_from_exposed_port_url() {
        let value = json!({
            "ports": [
                {
                    "containerPort": 8080,
                    "publicUrl": "https://web.example.com"
                },
                {
                    "containerPort": 50051,
                    "publicUrl": "https://runner.example.com"
                }
            ]
        });

        assert_eq!(
            extract_agentenv_runner_endpoint(&value, Some(50051)).as_deref(),
            Some("https://runner.example.com")
        );
    }

    #[test]
    fn builds_agentenv_runner_endpoint_from_host_and_port() {
        let value = json!({
            "exposedPorts": [
                {
                    "containerPort": "50051",
                    "host": "runner.agentenv.example",
                    "publicPort": 443,
                    "scheme": "https"
                }
            ]
        });

        assert_eq!(
            extract_agentenv_runner_endpoint(&value, Some(50051)).as_deref(),
            Some("https://runner.agentenv.example:443")
        );
    }

    #[test]
    fn builds_agentenv_runner_endpoint_from_configured_host_and_host_port() {
        let value = json!({
            "mappings": [
                {
                    "containerPort": 50051,
                    "protocol": "tcp",
                    "hostPort": 10145
                }
            ]
        });

        assert_eq!(
            extract_agentenv_runner_endpoint_with_host(&value, Some(50051), Some("93.115.25.198"))
                .as_deref(),
            Some("http://93.115.25.198:10145")
        );
    }

    #[test]
    fn ssh_runner_remote_command_starts_tool_runner_with_token_and_cwd() {
        let command = ssh_runner_remote_command(50123, Some("tok'en"), "/workspace/project");

        assert!(command.contains("PUFFER_RUNNER_TOKEN='tok'\"'\"'en'"));
        assert!(command.contains("exec puffer-tool-runner"));
        assert!(command.contains("--bind '127.0.0.1:50123'"));
        assert!(command.contains("--cwd '/workspace/project'"));
    }

    #[test]
    fn ssh_runner_remote_command_defaults_to_unauthenticated_runner() {
        let command = ssh_runner_remote_command(50124, None, "~/repo");

        assert!(command.starts_with("exec puffer-tool-runner"));
        assert!(command.contains("--bind '127.0.0.1:50124'"));
        assert!(command.contains("--cwd '~/repo'"));
        assert!(command.contains("--allow-unauthenticated"));
        assert!(!command.contains("PUFFER_RUNNER_TOKEN"));
    }

    #[test]
    fn ssh_runner_remote_command_uses_windows_shell_for_windows_cwd() {
        let command = ssh_runner_remote_command(50125, None, "C:\\Users\\walde");

        assert!(command.starts_with("(puffer-tool-runner.exe"));
        assert!(command.contains("--bind \"127.0.0.1:50125\""));
        assert!(command.contains("--cwd \"C:\\Users\\walde\""));
        assert!(command.contains("\"%USERPROFILE%\\.cargo\\bin\\puffer-tool-runner.exe\""));
        assert!(command.contains("--allow-unauthenticated"));
        assert!(!command.contains("exec "));
    }

    #[test]
    fn ssh_runner_remote_command_sets_windows_token() {
        let command = ssh_runner_remote_command(50126, Some("tok"), "C:\\Users\\walde");

        assert!(command.starts_with("set PUFFER_RUNNER_TOKEN=\"tok\"&& (puffer-tool-runner.exe"));
        assert!(command.contains("--bind \"127.0.0.1:50126\""));
        assert!(command.contains("--cwd \"C:\\Users\\walde\""));
        assert!(command.contains("\"%USERPROFILE%\\.cargo\\bin\\puffer-tool-runner.exe\""));
        assert!(!command.contains("--allow-unauthenticated"));
    }

    #[test]
    fn ssh_port_one_is_rejected() {
        let error = validate_ssh_port(Some(1)).unwrap_err();

        assert!(error.to_string().contains("SSH port 1 is invalid"));
    }

    #[test]
    fn transport_errors_are_reconnectable() {
        assert!(remote_error_is_reconnectable(&anyhow::anyhow!(
            "transport error: tcp connect error"
        )));
        assert!(remote_error_is_reconnectable(&anyhow::anyhow!(
            "status: Unavailable, message: connection reset"
        )));
        assert!(!remote_error_is_reconnectable(&anyhow::anyhow!(
            "tool execution failed: command exited 1"
        )));
    }

    #[test]
    fn reconnect_active_ssh_remote_reports_missing_host_config() {
        let mut state = temp_state();
        let cwd = state.cwd.clone();
        state.active_remote_target = Some(ActiveRemoteTarget {
            kind: "ssh".to_string(),
            id: "missing-host".to_string(),
        });
        state.remote_session_status = Some("connected".to_string());
        state.remote_session_url = Some("http://127.0.0.1:50123".to_string());

        let error = reconnect_active_remote_runner(&mut state, &cwd).unwrap_err();

        assert!(error
            .to_string()
            .contains("cannot reconnect SSH remote `missing-host`: host config is missing"));
    }

    #[test]
    fn enter_remote_finds_ssh_host_saved_by_previous_turn() {
        let _guard = puffer_home_lock();
        let home = tempfile::tempdir().unwrap();
        let _home = ScopedPufferHome::set(home.path());
        let mut state = temp_state();
        let cwd = state.cwd.clone();
        let paths = ConfigPaths::discover(&cwd);
        let mut saved = state.config.clone();
        saved.remote.ssh_hosts.push(SshHostConfig {
            id: "windows-test".to_string(),
            label: "Windows Test".to_string(),
            target: "walde@10.0.0.8".to_string(),
            port: None,
            cwd: Some("C:\\Users\\walde".to_string()),
            auth_secret_id: None,
        });
        save_user_config(&paths, &saved).unwrap();

        let host = find_ssh_host(&mut state, &cwd, "windows-test")
            .unwrap()
            .expect("host");

        assert_eq!(host.target, "walde@10.0.0.8");
        assert_eq!(host.cwd.as_deref(), Some("C:\\Users\\walde"));
        assert_eq!(state.config.remote.ssh_hosts.len(), 1);
    }

    #[test]
    fn enter_remote_prefers_latest_saved_ssh_host_over_stale_memory() {
        let _guard = puffer_home_lock();
        let home = tempfile::tempdir().unwrap();
        let _home = ScopedPufferHome::set(home.path());
        let mut state = temp_state();
        state.config.remote.ssh_hosts.push(SshHostConfig {
            id: "windows-test".to_string(),
            label: "Windows Test".to_string(),
            target: "walde@10.0.0.8".to_string(),
            port: Some(1),
            cwd: Some("C:\\Users\\walde".to_string()),
            auth_secret_id: None,
        });
        let cwd = state.cwd.clone();
        let paths = ConfigPaths::discover(&cwd);
        let mut saved = state.config.clone();
        saved.remote.ssh_hosts[0].port = Some(22);
        save_user_config(&paths, &saved).unwrap();

        let host = find_ssh_host(&mut state, &cwd, "windows-test")
            .unwrap()
            .expect("host");

        assert_eq!(host.port, Some(22));
        assert_eq!(state.config.remote.ssh_hosts[0].port, Some(22));
    }

    #[test]
    fn enter_remote_switches_runner_and_exit_restores_it() {
        let mut state = temp_state();
        state.config.remote.ssh_hosts.push(SshHostConfig {
            id: "devbox".to_string(),
            label: "Dev Box".to_string(),
            target: "walden@example.test".to_string(),
            port: Some(22),
            cwd: Some("/workspace/project".to_string()),
            auth_secret_id: None,
        });
        let original_runner = state.tool_runner.clone();
        let cwd = state.cwd.clone();

        let output = execute_remote_execution(
            &mut state,
            &cwd,
            json!({
                "action": "enter-remote",
                "targetType": "ssh",
                "hostId": "devbox",
                "runnerEndpoint": "http://127.0.0.1:1",
                "runnerAuthToken": "test-token",
                "waitForReady": false
            }),
        )
        .unwrap();

        assert!(output.contains("\"runnerCwd\": \"/workspace/project\""));
        assert_eq!(
            state.active_remote_target,
            Some(ActiveRemoteTarget {
                kind: "ssh".to_string(),
                id: "devbox".to_string(),
            })
        );
        assert_eq!(
            state.active_remote_cwd,
            Some(PathBuf::from("/workspace/project"))
        );
        assert_eq!(
            state.active_remote_runner_auth_token.as_deref(),
            Some("test-token")
        );
        assert!(!Arc::ptr_eq(&state.tool_runner, &original_runner));

        execute_remote_execution(&mut state, &cwd, json!({ "action": "exit-remote" })).unwrap();

        assert_eq!(state.active_remote_target, None);
        assert_eq!(state.active_remote_cwd, None);
        assert_eq!(state.active_remote_runner_auth_token, None);
        assert!(Arc::ptr_eq(&state.tool_runner, &original_runner));
    }

    #[test]
    fn enter_agentenv_remote_defaults_to_container_user_cwd() {
        let mut state = temp_state();
        let cwd = state.cwd.clone();

        let output = execute_remote_execution(
            &mut state,
            &cwd,
            json!({
                "action": "enter-remote",
                "targetType": "agentenv",
                "sandboxId": "sandbox-123",
                "runnerEndpoint": "http://127.0.0.1:1",
                "runnerAuthToken": "test-token",
                "waitForReady": false
            }),
        )
        .unwrap();

        assert!(output.contains("\"runnerCwd\": \"/user\""));
        assert_eq!(state.active_remote_cwd, Some(PathBuf::from("/user")));
    }
}
