use super::ProcessWorkflowRunner;
use crate::daemon_workflow_backend_settings::save_workflow_backend_settings;
use crate::daemon_workflow_backend_settings::test_support::{
    lock_secret_store, temp_paths, ScopedSecretStoreKey,
};
use crate::desktop_api_types::SaveWorkflowBackendSettingsParams;
use puffer_config::{ensure_workspace_dirs, PufferConfig, WorkflowBackendMode};
use puffer_provider_registry::{AuthStore, ProviderRegistry};
use puffer_resources::LoadedResources;
use puffer_subscriptions::WorkflowActionRunner;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct CapturedRequest {
    head: String,
    body: String,
}

#[test]
fn process_runner_executes_agentenv_workflow_with_trigger_input_body() {
    let _guard = lock_secret_store();
    let _secret_store_key = ScopedSecretStoreKey::set();
    let temp = tempfile::tempdir().expect("tempdir");
    let paths = temp_paths(&temp);
    ensure_workspace_dirs(&paths).expect("workspace dirs");
    let (api_url, requests, handle) = spawn_execute_server();
    let mut config = PufferConfig::default();
    save_workflow_backend_settings(
        &paths,
        &mut config,
        SaveWorkflowBackendSettingsParams {
            mode: WorkflowBackendMode::AgentEnvCloud,
            api_url,
            ui_url: "http://localhost:5173".to_string(),
            workspace_id: "workspace-agentenv".to_string(),
            api_token: Some("runtime-token".to_string()),
            keep_token: false,
        },
    )
    .expect("save backend settings");
    let runner = ProcessWorkflowRunner {
        paths: paths.clone(),
        config: PufferConfig::default(),
        resources: LoadedResources::default(),
        providers: ProviderRegistry::new(),
        auth_store: AuthStore::default(),
        lock: Mutex::new(()),
    };
    let workflow_id = "Wf_01HX.Runtime-123";
    let trigger = json!({
        "type": "connection",
        "envelope_id": "env-1",
        "connection_id": "telegram-user",
        "receivedAt": "2023-11-14T22:13:20Z",
        "topic": "telegram-user",
        "kind": "message",
        "dedup_key": "telegram-user:42",
        "text": "ship it",
        "payload": {
            "chat_id": 42
        }
    });

    runner
        .run_workflow(workflow_id, trigger.clone())
        .expect("run workflow");
    handle.join().expect("runtime server joined");

    let captured = requests.lock().expect("requests lock");
    assert_eq!(captured.len(), 1);
    assert!(captured[0]
        .head
        .starts_with("POST /v1/workflows/Wf_01HX.Runtime-123/execute "));
    assert!(captured[0]
        .head
        .to_ascii_lowercase()
        .contains("x-api-key: runtime-token"));
    assert!(captured[0]
        .head
        .to_ascii_lowercase()
        .contains("x-workspace-id: workspace-agentenv"));
    let body: Value = serde_json::from_str(&captured[0].body).expect("request body JSON");
    assert_eq!(body, json!({ "input": trigger }));
}

fn spawn_execute_server() -> (
    String,
    Arc<Mutex<Vec<CapturedRequest>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test runtime");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept runtime request");
        let request = read_request(&mut stream);
        captured.lock().expect("requests lock").push(request);
        write_json_response(
            &mut stream,
            r#"{"data":{"executionId":"exec-1","status":"completed"}}"#,
        );
    });
    (url, requests, handle)
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut buffer).expect("read request");
        assert!(read > 0, "request ended before headers");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let head = String::from_utf8(bytes[..header_end].to_vec()).expect("request head utf8");
    let content_length = content_length(&head);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).expect("read request body");
        assert!(read > 0, "request ended before body");
        bytes.extend_from_slice(&buffer[..read]);
    }
    let body = String::from_utf8(bytes[header_end..header_end + content_length].to_vec())
        .expect("request body utf8");
    CapturedRequest { head, body }
}

fn content_length(head: &str) -> usize {
    head.lines()
        .filter_map(|line| line.split_once(':'))
        .find_map(|(name, value)| {
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("content length"))
        })
        .unwrap_or(0)
}

fn write_json_response(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
}
