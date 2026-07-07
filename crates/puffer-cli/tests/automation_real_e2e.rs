use serde_json::{json, Value};
use std::io::{ErrorKind, Read};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket};
use url::Url;

const API_URL_ENV: &str = "PUFFER_WORKFLOW_API_URL";
const WORKSPACE_ID_ENV: &str = "PUFFER_WORKFLOW_WORKSPACE_ID";
const API_TOKEN_ENV: &str = "PUFFER_WORKFLOW_API_TOKEN";
const MODE_ENV: &str = "PUFFER_AUTOMATION_E2E_MODE";
const RUNTIME_PROJECT_ENV: &str = "PUFFER_WORKFLOW_RUNTIME_PROJECT";
const TEST_SECRET_STORE_KEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

#[test]
#[ignore = "requires local Docker runtime or real AgentEnv Cloud credentials"]
fn automation_real_e2e_compile_deploy_execute_preview() {
    let mode = AutomationE2eMode::from_env();

    let tempdir = tempfile::tempdir().expect("tempdir");
    let workspace = tempdir.path().join("workspace");
    let puffer_home = tempdir.path().join("home");
    let puffer_config = puffer_home.join(".puffer");
    let runtime_project = format!(
        "puffer-workflow-runtime-e2e-{}-{}",
        std::process::id(),
        unix_timestamp_ms()
    );
    let _local_runtime_cleanup = LocalRuntimeCleanup::new(
        matches!(mode, AutomationE2eMode::Local),
        &puffer_config,
        &runtime_project,
    );
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&puffer_config).expect("puffer config");
    let discovery_cache = tempdir.path().join("discovery.json");
    std::fs::write(&discovery_cache, discovery_cache_json()).expect("discovery cache");

    let mut extra_env = vec![("PUFFER_SECRET_STORE_KEY", TEST_SECRET_STORE_KEY)];
    if matches!(mode, AutomationE2eMode::Local) {
        extra_env.push((RUNTIME_PROJECT_ENV, runtime_project.as_str()));
    }
    let mut daemon =
        DaemonProcess::start_with_env(&workspace, &puffer_home, &discovery_cache, &extra_env);
    let mut client = DaemonClient::connect(&daemon.handshake);

    match &mode {
        AutomationE2eMode::Local => {
            let config = client.rpc("workflow_backend_get_config", json!({}));
            assert_eq!(config["mode"], "local");
        }
        AutomationE2eMode::Cloud(env) => {
            let saved_backend = client.rpc(
                "workflow_backend_save_config",
                json!({
                    "mode": "agent_env_cloud",
                    "apiUrl": env.api_url,
                    "uiUrl": "https://agentenv.io",
                    "workspaceId": env.workspace_id,
                    "apiToken": env.api_token,
                    "keepToken": false,
                }),
            );
            assert_eq!(saved_backend["hasToken"], true);
            let saved_backend_text = saved_backend.to_string();
            assert!(!saved_backend_text.contains("apiToken"));
            assert!(!saved_backend_text.contains("api_token"));
        }
    }

    let automation_id = format!("automation-real-e2e-{}", unix_timestamp_ms());
    let spec = automation_spec(&automation_id);
    let saved = client.rpc(
        "automation_save",
        json!({
            "id": automation_id,
            "status": "enabled",
            "spec": spec,
        }),
    );
    let revision = saved["revision"].as_u64().expect("saved revision");

    let deployed = client.rpc_with_mode(
        &mode,
        "automation_compile_deploy",
        json!({
            "id": saved["id"],
            "expectedRevision": revision,
        }),
    );
    assert_eq!(deployed["runtime"]["status"], "deployed");
    assert_eq!(deployed["runtime"]["compiled_revision"], revision);

    let preview = client.rpc_with_mode(
        &mode,
        "automation_run_preview",
        json!({
            "id": saved["id"],
            "input": {
                "source": "automation-real-e2e",
                "timestamp_ms": unix_timestamp_ms()
            }
        }),
    );
    assert_eq!(preview["status"], "completed");
    assert_eq!(preview["runtime"]["status"], "deployed");
    assert_eq!(preview["runtime"]["compiled_revision"], revision);
    assert!(
        preview["runtime"]["agentenv_workflow_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_public_preview_response(&preview);

    let fetched = client.rpc("automation_get", json!({ "id": saved["id"] }));
    assert_eq!(fetched["runtime"]["status"], "deployed");
    assert_eq!(fetched["runtime"]["compiled_revision"], revision);
    assert!(
        fetched["runtime"]["agentenv_workflow_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_public_preview_response(&fetched);

    daemon.stop();
}

enum AutomationE2eMode {
    Local,
    Cloud(AutomationE2eCloudEnv),
}

impl AutomationE2eMode {
    fn from_env() -> Self {
        match env_trimmed(MODE_ENV).as_deref() {
            Some("cloud") => Self::Cloud(AutomationE2eCloudEnv::from_env()),
            Some("local") | None => Self::Local,
            Some(other) => panic!("{MODE_ENV} must be `local` or `cloud`, got `{other}`"),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud(_) => "cloud",
        }
    }
}

struct AutomationE2eCloudEnv {
    api_url: String,
    workspace_id: String,
    api_token: String,
}

impl AutomationE2eCloudEnv {
    fn from_env() -> Self {
        let missing = [API_URL_ENV, WORKSPACE_ID_ENV, API_TOKEN_ENV]
            .into_iter()
            .filter(|name| env_trimmed(name).is_none())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            panic!(
                "cloud Automation e2e requires missing environment variable(s): {}",
                missing.join(", ")
            );
        }
        Self {
            api_url: env_trimmed(API_URL_ENV).expect("api url"),
            workspace_id: env_trimmed(WORKSPACE_ID_ENV).expect("workspace id"),
            api_token: env_trimmed(API_TOKEN_ENV).expect("api token"),
        }
    }
}

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn automation_spec(automation_id: &str) -> Value {
    json!({
        "spec_version": 1,
        "name": format!("Automation real e2e {automation_id}"),
        "source": { "type": "blank" },
        "instructions": "Run a minimal real AgentEnv workflow from Puffer Automation preview.",
        "triggers": [
            {
                "type": "agent_env_node",
                "id": "smoke-webhook",
                "node": {
                    "node_type": "webhook",
                    "name": "Smoke webhook",
                    "trusted": false,
                    "config": {
                        "path": automation_id,
                        "methods": ["POST"],
                        "authentication": "none"
                    }
                }
            }
        ],
        "flow": {
            "steps": [
                {
                    "type": "agent_env_node",
                    "id": "transform",
                    "node": {
                        "node_type": "transform_js",
                        "name": "Transform",
                        "trusted": true,
                        "config": transform_config()
                    }
                }
            ]
        },
        "review": {
            "human_approval_required": true
        }
    })
}

fn transform_config() -> Value {
    json!({ "code": "return { ok: true, input };" })
}

fn assert_public_preview_response(value: &Value) {
    let text = value.to_string();
    assert!(!text.contains("workflowId"));
    assert!(!text.contains("workflow_id"));
    assert!(!text.contains("workflowSlug"));
    assert!(!text.contains("workflow_slug"));
    assert!(!text.contains("bindingSlug"));
    assert!(!text.contains("binding_slug"));
}

fn friendly_rpc_error(mode: &AutomationE2eMode, error: &Value) -> String {
    let text = error.to_string();
    if matches!(mode, AutomationE2eMode::Local) {
        if text.contains("docker_missing") {
            return format!("Docker is not running. Raw daemon error: {text}");
        }
        if text.contains("image_missing") {
            return format!(
                "Local AgentEnv runtime image agentenv/api-server:local is missing. Raw daemon error: {text}"
            );
        }
    }
    text
}

struct DaemonProcess {
    child: Child,
    handshake: Value,
    stderr: Arc<Mutex<String>>,
}

struct LocalRuntimeCleanup {
    enabled: bool,
    compose_file: std::path::PathBuf,
    project_name: String,
}

impl LocalRuntimeCleanup {
    fn new(enabled: bool, puffer_config: &Path, project_name: &str) -> Self {
        Self {
            enabled,
            compose_file: puffer_config
                .join("workflow-runtime")
                .join("docker-compose.yml"),
            project_name: project_name.to_string(),
        }
    }
}

impl Drop for LocalRuntimeCleanup {
    fn drop(&mut self) {
        if !self.enabled || !self.compose_file.exists() {
            return;
        }
        let _ = Command::new("docker")
            .args([
                "compose",
                "-f",
                self.compose_file.to_string_lossy().as_ref(),
                "-p",
                &self.project_name,
                "down",
                "-v",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl DaemonProcess {
    fn start_with_env(
        workspace: &Path,
        puffer_home: &Path,
        discovery_cache: &Path,
        extra_env: &[(&str, &str)],
    ) -> Self {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("cli crate parent")
            .parent()
            .expect("repo root");
        let mut command = Command::new(env!("CARGO_BIN_EXE_puffer"));
        command
            .args([
                "daemon",
                "--bind",
                "127.0.0.1:0",
                "--token",
                "automation-smoke-token",
                "--print-handshake",
                "--no-browser",
                "--disable-auto-title",
            ])
            .current_dir(workspace)
            .env("PUFFER_HOME", puffer_home)
            .env("PUFFER_BUILTIN_RESOURCES_DIR", repo_root.join("resources"))
            .env("PUFFER_DISCOVERY_CACHE_PATH", discovery_cache)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn daemon");

        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_thread = Arc::clone(&stderr);
        let mut err = child.stderr.take().expect("daemon stderr");
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = err.read_to_string(&mut buf);
            *stderr_thread.lock().unwrap() = buf;
        });

        let mut stdout = child.stdout.take().expect("daemon stdout");
        let handshake = read_handshake_line(&mut stdout, &mut child, &stderr);
        Self {
            child,
            handshake,
            stderr,
        }
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let stderr = self.stderr.lock().unwrap();
        if !stderr.is_empty() {
            eprintln!("daemon stderr:\n{stderr}");
        }
    }
}

struct DaemonClient {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    backlog: Vec<Value>,
}

impl DaemonClient {
    fn connect(handshake: &Value) -> Self {
        let mut url = Url::parse(handshake["url"].as_str().expect("daemon url")).expect("url");
        url.query_pairs_mut()
            .append_pair("token", handshake["token"].as_str().expect("token"));
        let (socket, _) = connect(url.as_str()).expect("connect daemon websocket");
        set_daemon_socket_read_timeout(&socket, Some(Duration::from_millis(100)));
        Self {
            socket,
            next_id: 1,
            backlog: Vec::new(),
        }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.rpc_with_context(method, params, None)
    }

    fn rpc_with_mode(&mut self, mode: &AutomationE2eMode, method: &str, params: Value) -> Value {
        self.rpc_with_context(method, params, Some(mode))
    }

    fn rpc_with_context(
        &mut self,
        method: &str,
        params: Value,
        mode: Option<&AutomationE2eMode>,
    ) -> Value {
        let message = self.rpc_response(method, params);
        if message["error"].is_null() {
            message["result"].clone()
        } else {
            if let Some(mode) = mode {
                panic!(
                    "{method} failed in {} mode: {}",
                    mode.name(),
                    friendly_rpc_error(mode, &message["error"])
                );
            }
            panic!("{method} failed: {}", message["error"]);
        }
    }

    fn rpc_response(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id.to_string();
        self.next_id += 1;
        self.socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .expect("send daemon request");
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            assert!(Instant::now() < deadline, "{method} timed out");
            let message = self.read_message_until(deadline);
            if message["id"].as_str() == Some(id.as_str()) {
                return message;
            }
            self.backlog.push(message);
        }
    }

    fn read_message_until(&mut self, deadline: Instant) -> Value {
        loop {
            assert!(Instant::now() < deadline, "daemon message timed out");
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(&text).expect("daemon message json");
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(error) => panic!("read daemon message: {error}"),
            }
        }
    }
}

fn read_handshake_line(
    stdout: &mut impl Read,
    child: &mut Child,
    stderr: &Arc<Mutex<String>>,
) -> Value {
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut buf = [0_u8; 1];
    while Instant::now() < deadline {
        match stdout.read(&mut buf) {
            Ok(0) => {
                if let Some(status) = child.try_wait().expect("daemon status") {
                    panic!(
                        "daemon exited before handshake: {status}\n{}",
                        stderr.lock().unwrap()
                    );
                }
                thread::sleep(Duration::from_millis(10));
            }
            Ok(_) if buf[0] == b'\n' => break,
            Ok(_) => line.push(buf[0] as char),
            Err(error) => panic!("read daemon handshake: {error}"),
        }
    }
    assert!(!line.is_empty(), "daemon handshake timed out");
    serde_json::from_str(&line).expect("handshake json")
}

fn set_daemon_socket_read_timeout(
    socket: &WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Option<Duration>,
) {
    let tcp = match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => stream.get_ref(),
        _ => return,
    };
    let _ = tcp.set_read_timeout(timeout);
}

fn discovery_cache_json() -> String {
    let now = 1_700_000_000_000_u64;
    json!({
        "entries": {
            "llama-cpp": { "models": [], "cached_at_ms": now },
            "lmstudio": { "models": [], "cached_at_ms": now },
            "ollama": { "models": [], "cached_at_ms": now },
            "vllm": { "models": [], "cached_at_ms": now }
        }
    })
    .to_string()
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis()
}
