//! Server side of the puffer gRPC tool runner.
//!
//! [`ToolRunnerService`] is a thin adapter: every RPC forwards to the
//! supplied `Arc<dyn ToolRunner>`. There is no business logic here; if a
//! tool's behaviour needs to change, fix it in the underlying runner.
//!
//! The `puffer-tool-runner` binary constructs one of these wrapping a
//! [`puffer_runner_api::ToolRunner`] (typically `LocalToolRunner`) and hands
//! it to `tonic::transport::Server`. Integration tests do the same in-process.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use prost::Message;
use puffer_runner_api::{
    ChunkKind, ChunkSink, ElicitationHandler, ElicitationMode, ElicitationRequest,
    ElicitationResponse, FnChunkSink, RunnerError, ToolRequest, ToolRunner,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::convert::{
    from_proto_tool_request, oauth_payload_from_proto, oauth_status_to_proto,
    runner_error_to_status, to_proto_capabilities, to_proto_dir_entry, to_proto_mcp_prompt,
    to_proto_mcp_prompt_content, to_proto_mcp_resource_content, to_proto_mcp_resource_record,
    to_proto_mcp_result, to_proto_mcp_server, to_proto_mcp_tool, to_proto_tool_completed,
};
use crate::proto;
use crate::AUTH_METADATA_KEY;

pub use proto::tool_runner_server::ToolRunnerServer;

const MAX_IDEMPOTENT_EXECUTIONS: usize = 512;

/// Adapter from a synchronous `Arc<dyn ToolRunner>` to the generated tonic
/// service trait. All RPCs forward to the runner; blocking work runs on a
/// `spawn_blocking` thread to avoid stalling the tonic worker pool.
///
/// ## Elicitation routing
///
/// `BidiElicitationRouter` is installed at construction so server-initiated
/// MCP `elicitation/create` requests can be forwarded to whichever
/// `CallMcpTool` bidi stream triggered them. The router keys active calls
/// by `(server, call_id)` and matches incoming `elicit()` invocations by
/// MCP server label, picking the most recent registration. This is correct
/// for the common case (one `CallMcpTool` per gRPC stream) and degrades
/// gracefully under concurrency: simultaneous calls to the same MCP server
/// will see elicitations routed to whichever call registered most recently;
/// the older call's elicitations would be dropped. We accept that trade-off
/// for Pass 1.5c — concurrent calls to the same MCP server are rare in
/// practice, and the alternative (per-call rmcp respawn) is far too costly.
#[derive(Clone)]
pub struct ToolRunnerService {
    runner: Arc<dyn ToolRunner>,
    auth_token: Option<Arc<String>>,
    started: Instant,
    elicitation_router: Arc<BidiElicitationRouter>,
    idempotent_executions: Arc<Mutex<HashMap<String, Arc<IdempotentExecution>>>>,
    idempotency_store: Option<Arc<IdempotencyStore>>,
}

impl ToolRunnerService {
    /// Wraps `runner` in a tonic-ready service. No auth token.
    ///
    /// MCP server-initiated `elicitation/create` requests from this
    /// service's `CallMcpTool` streams will be honored only if the
    /// `runner` was built with the matching router from
    /// [`build_runner_router`] installed via
    /// [`puffer_runner_api::ElicitationHandler`]. Without that hook, they
    /// fall back to whatever default the runner has (typically
    /// `DeclineAllElicitations`).
    pub fn new(runner: Arc<dyn ToolRunner>) -> Self {
        Self::with_router(runner, Arc::new(BidiElicitationRouter::default()))
    }

    /// Like [`ToolRunnerService::new`] but takes an externally-built router
    /// so the caller can install it on the underlying runner before
    /// constructing the service. Pair with [`build_router`] (returns a
    /// fresh router) when you need to wire both sides together.
    pub fn with_router(runner: Arc<dyn ToolRunner>, router: Arc<BidiElicitationRouter>) -> Self {
        Self {
            runner,
            auth_token: None,
            started: Instant::now(),
            elicitation_router: router,
            idempotent_executions: Arc::new(Mutex::new(HashMap::new())),
            idempotency_store: None,
        }
    }

    /// Returns the elicitation router that matches this service's
    /// `CallMcpTool` bidi streams. Install it on the underlying runner so
    /// MCP server-initiated `elicitation/create` requests get forwarded to
    /// the matching gRPC client.
    pub fn elicitation_router(&self) -> Arc<dyn ElicitationHandler> {
        Arc::clone(&self.elicitation_router) as Arc<dyn ElicitationHandler>
    }

    /// Requires every RPC to carry `authorization: Bearer <token>` metadata
    /// matching `token`. Calls without the header are rejected with
    /// `Unauthenticated`.
    pub fn with_auth_token(mut self, token: Option<String>) -> Self {
        self.auth_token = token.map(Arc::new);
        self
    }

    /// Persists completed idempotent tool executions so duplicate
    /// `request_id` calls can be replayed after a runner process restart.
    pub fn with_idempotency_dir(mut self, dir: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        self.idempotency_store = Some(Arc::new(IdempotencyStore::open(dir.into())?));
        Ok(self)
    }

    fn check_auth<T>(&self, req: &Request<T>) -> Result<(), Status> {
        let Some(expected) = self.auth_token.as_deref() else {
            return Ok(());
        };
        let Some(value) = req.metadata().get(AUTH_METADATA_KEY) else {
            return Err(Status::unauthenticated("missing authorization metadata"));
        };
        let raw = value
            .to_str()
            .map_err(|_| Status::unauthenticated("non-ASCII authorization metadata"))?;
        let token = raw.strip_prefix("Bearer ").unwrap_or(raw);
        if token == expected.as_str() {
            Ok(())
        } else {
            Err(Status::unauthenticated("invalid bearer token"))
        }
    }

    fn execute_tool_uncached(
        &self,
        request: ToolRequest,
        event_tx: mpsc::Sender<Result<proto::ToolEvent, Status>>,
    ) {
        let runner = self.runner.clone();
        let event_tx_for_blocking = event_tx.clone();

        tokio::task::spawn_blocking(move || {
            let mut sink = ChannelChunkSink::new(event_tx_for_blocking.clone());
            send_tool_execution_result(&runner, request, &mut sink, &event_tx_for_blocking, None);
        });
        drop(event_tx);
    }

    fn execute_tool_idempotently(
        &self,
        request_id: String,
        request: ToolRequest,
        event_tx: mpsc::Sender<Result<proto::ToolEvent, Status>>,
    ) -> Result<(), Status> {
        let fingerprint = request_fingerprint(&request)?;
        if let Some(store) = &self.idempotency_store {
            if let Some(replay) = store.load(&request_id).map_err(idempotency_store_error)? {
                if replay.fingerprint != fingerprint {
                    return Err(Status::already_exists(
                        "request_id was already used for a different tool request",
                    ));
                }
                tokio::task::spawn_blocking(move || {
                    for event in replay.events {
                        let _ = event_tx.blocking_send(Ok(event));
                    }
                });
                return Ok(());
            }
        }
        let (execution, should_start) = {
            let mut executions = self
                .idempotent_executions
                .lock()
                .map_err(|_| Status::internal("idempotency cache lock poisoned"))?;
            match executions.get(&request_id) {
                Some(existing) => {
                    if existing.fingerprint != fingerprint {
                        return Err(Status::already_exists(
                            "request_id was already used for a different tool request",
                        ));
                    }
                    (existing.clone(), false)
                }
                None => {
                    if executions.len() >= MAX_IDEMPOTENT_EXECUTIONS {
                        if let Some(evict) = executions
                            .iter()
                            .find_map(|(id, execution)| execution.is_complete().then(|| id.clone()))
                        {
                            executions.remove(&evict);
                        }
                    }
                    let execution = Arc::new(IdempotentExecution::new(fingerprint));
                    executions.insert(request_id.clone(), execution.clone());
                    (execution, true)
                }
            }
        };

        if should_start {
            let runner = self.runner.clone();
            let event_tx_for_blocking = event_tx.clone();
            let store = self.idempotency_store.clone();
            let persist_request_id = request_id.clone();
            tokio::task::spawn_blocking(move || {
                let mut sink =
                    RecordingChunkSink::new(event_tx_for_blocking.clone(), execution.clone());
                send_tool_execution_result(
                    &runner,
                    request,
                    &mut sink,
                    &event_tx_for_blocking,
                    Some(&execution),
                );
                execution.complete();
                if let Some(store) = store {
                    let events = execution.wait_for_replay();
                    if let Err(error) =
                        store.save(&persist_request_id, &execution.fingerprint, &events)
                    {
                        eprintln!("puffer-tool-runner: idempotency persist failed: {error}");
                    }
                }
            });
            drop(event_tx);
        } else {
            tokio::task::spawn_blocking(move || {
                for event in execution.wait_for_replay() {
                    let _ = event_tx.blocking_send(Ok(event));
                }
            });
        }

        Ok(())
    }
}

#[tonic::async_trait]
impl proto::tool_runner_server::ToolRunner for ToolRunnerService {
    type ExecuteToolStream = ReceiverStream<Result<proto::ToolEvent, Status>>;
    type CallMcpToolStream = ReceiverStream<Result<proto::McpToolMessage, Status>>;

    async fn ping(
        &self,
        req: Request<proto::Empty>,
    ) -> Result<Response<proto::PingResponse>, Status> {
        self.check_auth(&req)?;
        Ok(Response::new(proto::PingResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.started.elapsed().as_secs(),
        }))
    }

    async fn capabilities(
        &self,
        req: Request<proto::Empty>,
    ) -> Result<Response<proto::RunnerCapabilities>, Status> {
        self.check_auth(&req)?;
        let runner = self.runner.clone();
        let caps = tokio::task::spawn_blocking(move || runner.capabilities())
            .await
            .map_err(internal_join_error)?;
        Ok(Response::new(to_proto_capabilities(&caps)))
    }

    async fn execute_tool(
        &self,
        req: Request<proto::ToolRequest>,
    ) -> Result<Response<Self::ExecuteToolStream>, Status> {
        self.check_auth(&req)?;
        let proto_req = req.into_inner();
        let request = from_proto_tool_request(proto_req).map_err(|e| runner_error_to_status(&e))?;

        let (event_tx, event_rx) = mpsc::channel::<Result<proto::ToolEvent, Status>>(32);
        if let Some(request_id) = request.request_id.clone() {
            self.execute_tool_idempotently(request_id, request, event_tx)?;
        } else {
            self.execute_tool_uncached(request, event_tx);
        }

        Ok(Response::new(ReceiverStream::new(event_rx)))
    }

    async fn read_file(
        &self,
        req: Request<proto::ReadFileRequest>,
    ) -> Result<Response<proto::FileContent>, Status> {
        self.check_auth(&req)?;
        let path = std::path::PathBuf::from(req.into_inner().path);
        let runner = self.runner.clone();
        let bytes = tokio::task::spawn_blocking(move || runner.read_file(&path))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::FileContent { data: bytes }))
    }

    async fn list_dir(
        &self,
        req: Request<proto::ListDirRequest>,
    ) -> Result<Response<proto::DirListing>, Status> {
        self.check_auth(&req)?;
        let path = std::path::PathBuf::from(req.into_inner().path);
        let runner = self.runner.clone();
        let entries = tokio::task::spawn_blocking(move || runner.list_dir(&path))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::DirListing {
            entries: entries.iter().map(to_proto_dir_entry).collect(),
        }))
    }

    async fn glob(
        &self,
        req: Request<proto::GlobRequest>,
    ) -> Result<Response<proto::GlobResponse>, Status> {
        self.check_auth(&req)?;
        let inner = req.into_inner();
        let root = std::path::PathBuf::from(inner.root);
        let pattern = inner.pattern;
        let runner = self.runner.clone();
        let paths = tokio::task::spawn_blocking(move || runner.glob(&root, &pattern))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::GlobResponse {
            paths: paths.iter().map(|p| p.display().to_string()).collect(),
        }))
    }

    async fn list_mcp_servers(
        &self,
        req: Request<proto::Empty>,
    ) -> Result<Response<proto::McpServerList>, Status> {
        self.check_auth(&req)?;
        let runner = self.runner.clone();
        let servers = tokio::task::spawn_blocking(move || runner.list_mcp_servers())
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::McpServerList {
            servers: servers.iter().map(to_proto_mcp_server).collect(),
        }))
    }

    async fn list_mcp_tools(
        &self,
        req: Request<proto::McpServerRef>,
    ) -> Result<Response<proto::McpToolList>, Status> {
        self.check_auth(&req)?;
        let server = req.into_inner().server;
        let runner = self.runner.clone();
        let tools = tokio::task::spawn_blocking(move || runner.list_mcp_tools(&server))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::McpToolList {
            tools: tools.iter().map(to_proto_mcp_tool).collect(),
        }))
    }

    async fn call_mcp_tool(
        &self,
        req: Request<Streaming<proto::McpToolMessage>>,
    ) -> Result<Response<Self::CallMcpToolStream>, Status> {
        self.check_auth(&req)?;
        let mut inbound = req.into_inner();

        // Pull the first message — must be the `Call` envelope.
        let first = inbound
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("CallMcpTool stream closed before Call"))?;
        let call = match first.payload {
            Some(proto::mcp_tool_message::Payload::Call(call)) => call,
            other => {
                return Err(Status::invalid_argument(format!(
                    "first CallMcpTool message must be Call, got {other:?}"
                )));
            }
        };
        let args: serde_json::Value = if call.args_json.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&call.args_json)
                .map_err(|e| Status::invalid_argument(format!("args_json: {e}")))?
        };

        // Outbound channel. Sender is cloned into the blocking worker
        // (events + completion) and into the route registered with the
        // elicitation router (for elicitation_request envelopes the router
        // emits when the MCP server triggers a `create_elicitation`).
        let (tx, rx) = mpsc::channel::<Result<proto::McpToolMessage, Status>>(32);

        // Per-call pending-elicitations map. The router pushes new entries
        // here when it emits an elicitation_request; the inbound loop
        // removes them when the client's response arrives.
        let pending: PendingElicitations = Arc::new(Mutex::new(HashMap::new()));
        let counter = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let route = Arc::new(BidiRoute {
            outbound: tx.clone(),
            pending: Arc::clone(&pending),
            counter: Arc::clone(&counter),
        });

        // Register with the service-wide router so this stream's MCP
        // server-initiated elicitations route back to us. Held by a guard
        // that auto-deregisters when the call completes (success or panic).
        let _route_guard = self
            .elicitation_router
            .register(call.server.clone(), Arc::clone(&route));

        // Inbound message loop: drains elicitation responses from the
        // client and routes them to the matching oneshot.
        let pending_for_inbound = Arc::clone(&pending);
        let inbound_task = tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                if let Some(proto::mcp_tool_message::Payload::ElicitationResponse(resp)) =
                    msg.payload
                {
                    let parsed = parse_elicitation_response(&resp);
                    if let Some(sender) = pending_for_inbound
                        .lock()
                        .ok()
                        .and_then(|mut map| map.remove(&resp.request_id))
                    {
                        let _ = sender.send(parsed);
                    }
                }
            }
            // Stream closed: drop pending oneshots so any blocked sync
            // handler unwedges with a default Decline.
            if let Ok(mut map) = pending_for_inbound.lock() {
                map.clear();
            }
        });

        let runner = self.runner.clone();
        let tx_blocking = tx.clone();
        let call_for_blocking = call.clone();
        let blocking_handle = tokio::task::spawn_blocking(move || {
            let mut sink = McpChannelMessageSink::new(tx_blocking.clone());
            match runner.call_mcp_tool(
                &call_for_blocking.server,
                &call_for_blocking.tool,
                args,
                &mut sink,
            ) {
                Ok(result) => {
                    let _ = tx_blocking.blocking_send(Ok(proto::McpToolMessage {
                        payload: Some(proto::mcp_tool_message::Payload::Completed(
                            to_proto_mcp_result(&result),
                        )),
                    }));
                }
                Err(err) => {
                    let _ = tx_blocking.blocking_send(Ok(proto::McpToolMessage {
                        payload: Some(proto::mcp_tool_message::Payload::Failed(
                            proto::ToolFailed {
                                code: runner_error_code(&err).to_string(),
                                message: err.to_string(),
                            },
                        )),
                    }));
                }
            }
        });
        drop(tx);

        // Coordinator task: keeps the inbound loop and the elicitation
        // route registered for the lifetime of the call. When the blocking
        // worker finishes, the route is dropped (auto-deregisters) and we
        // abort the inbound loop so the gRPC stream can terminate cleanly.
        tokio::spawn(async move {
            let _ = blocking_handle.await;
            inbound_task.abort();
            drop(_route_guard);
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn list_mcp_resources(
        &self,
        req: Request<proto::McpResourceQuery>,
    ) -> Result<Response<proto::McpResourceList>, Status> {
        self.check_auth(&req)?;
        let server = req.into_inner().server;
        let runner = self.runner.clone();
        let records =
            tokio::task::spawn_blocking(move || runner.list_mcp_resources(server.as_deref()))
                .await
                .map_err(internal_join_error)?
                .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::McpResourceList {
            resources: records.iter().map(to_proto_mcp_resource_record).collect(),
        }))
    }

    async fn read_mcp_resource(
        &self,
        req: Request<proto::McpResourceRef>,
    ) -> Result<Response<proto::McpResourceContent>, Status> {
        self.check_auth(&req)?;
        let inner = req.into_inner();
        let runner = self.runner.clone();
        let content = tokio::task::spawn_blocking(move || {
            runner.read_mcp_resource(&inner.server, &inner.uri)
        })
        .await
        .map_err(internal_join_error)?
        .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(to_proto_mcp_resource_content(&content)))
    }

    async fn list_mcp_prompts(
        &self,
        req: Request<proto::McpServerRef>,
    ) -> Result<Response<proto::McpPromptList>, Status> {
        self.check_auth(&req)?;
        let server = req.into_inner().server;
        let runner = self.runner.clone();
        let prompts = tokio::task::spawn_blocking(move || runner.list_mcp_prompts(&server))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::McpPromptList {
            prompts: prompts.iter().map(to_proto_mcp_prompt).collect(),
        }))
    }

    async fn push_o_auth_tokens(
        &self,
        req: Request<proto::PushOAuthTokensRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        self.check_auth(&req)?;
        let inner = req.into_inner();
        let server = inner.server;
        let payload = inner
            .tokens
            .ok_or_else(|| Status::invalid_argument("PushOAuthTokens missing tokens payload"))?;
        let tokens = oauth_payload_from_proto(payload);
        let runner = self.runner.clone();
        tokio::task::spawn_blocking(move || runner.push_oauth_tokens(&server, tokens))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::Empty {}))
    }

    async fn query_o_auth_status(
        &self,
        req: Request<proto::OAuthServerRef>,
    ) -> Result<Response<proto::OAuthStatusResponse>, Status> {
        self.check_auth(&req)?;
        let server = req.into_inner().server_id;
        let runner = self.runner.clone();
        let status = tokio::task::spawn_blocking(move || runner.oauth_status(&server))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(oauth_status_to_proto(status)))
    }

    async fn clear_o_auth_tokens(
        &self,
        req: Request<proto::OAuthServerRef>,
    ) -> Result<Response<proto::Empty>, Status> {
        self.check_auth(&req)?;
        let server = req.into_inner().server_id;
        let runner = self.runner.clone();
        tokio::task::spawn_blocking(move || runner.clear_oauth_tokens(&server))
            .await
            .map_err(internal_join_error)?
            .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(proto::Empty {}))
    }

    async fn get_mcp_prompt(
        &self,
        req: Request<proto::McpPromptRequest>,
    ) -> Result<Response<proto::McpPromptContent>, Status> {
        self.check_auth(&req)?;
        let inner = req.into_inner();
        let args: serde_json::Value = if inner.args_json.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&inner.args_json)
                .map_err(|e| Status::invalid_argument(format!("args_json: {e}")))?
        };
        let runner = self.runner.clone();
        let content = tokio::task::spawn_blocking(move || {
            runner.get_mcp_prompt(&inner.server, &inner.name, args)
        })
        .await
        .map_err(internal_join_error)?
        .map_err(|e| runner_error_to_status(&e))?;
        Ok(Response::new(to_proto_mcp_prompt_content(&content)))
    }
}

fn internal_join_error(e: tokio::task::JoinError) -> Status {
    Status::internal(format!("worker join: {e}"))
}

fn runner_error_code(err: &RunnerError) -> &'static str {
    match err {
        RunnerError::NotFound(_) => "not_found",
        RunnerError::PermissionDenied(_) => "permission_denied",
        RunnerError::Unsupported(_) => "unsupported",
        RunnerError::InvalidArgument(_) => "invalid_argument",
        RunnerError::Transport(_) => "transport",
        RunnerError::Mcp(_) => "mcp",
        RunnerError::OAuthRequired { .. } => "oauth_required",
        RunnerError::Execution(_) => "execution",
        RunnerError::Other(_) => "other",
    }
}

fn send_tool_execution_result(
    runner: &Arc<dyn ToolRunner>,
    request: ToolRequest,
    sink: &mut dyn ChunkSink,
    event_tx: &mpsc::Sender<Result<proto::ToolEvent, Status>>,
    record: Option<&IdempotentExecution>,
) {
    let event = match runner.execute_tool(request, sink) {
        Ok(result) => proto::ToolEvent {
            payload: Some(proto::tool_event::Payload::Completed(
                to_proto_tool_completed(&result),
            )),
        },
        Err(err) => proto::ToolEvent {
            payload: Some(proto::tool_event::Payload::Failed(proto::ToolFailed {
                code: runner_error_code(&err).to_string(),
                message: err.to_string(),
            })),
        },
    };
    if let Some(record) = record {
        record.record_event(event.clone());
    }
    let _ = event_tx.blocking_send(Ok(event));
}

fn request_fingerprint(request: &ToolRequest) -> Result<String, Status> {
    let mut request = request.clone();
    request.request_id = None;
    serde_json::to_string(&request)
        .map_err(|error| Status::internal(format!("request fingerprint: {error}")))
}

fn idempotency_store_error(error: std::io::Error) -> Status {
    Status::internal(format!("idempotency store: {error}"))
}

#[derive(Debug)]
struct IdempotencyStore {
    dir: PathBuf,
}

#[derive(Debug)]
struct PersistedReplay {
    fingerprint: String,
    events: Vec<proto::ToolEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedIdempotentExecution {
    fingerprint: String,
    event_frames: Vec<Vec<u8>>,
}

impl IdempotencyStore {
    fn open(dir: PathBuf) -> Result<Self, std::io::Error> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn load(&self, request_id: &str) -> Result<Option<PersistedReplay>, std::io::Error> {
        let path = self.path_for_request_id(request_id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        let persisted: PersistedIdempotentExecution =
            serde_json::from_slice(&bytes).map_err(invalid_data)?;
        let mut events = Vec::with_capacity(persisted.event_frames.len());
        for frame in persisted.event_frames {
            events.push(proto::ToolEvent::decode(frame.as_slice()).map_err(invalid_data)?);
        }
        Ok(Some(PersistedReplay {
            fingerprint: persisted.fingerprint,
            events,
        }))
    }

    fn save(
        &self,
        request_id: &str,
        fingerprint: &str,
        events: &[proto::ToolEvent],
    ) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.dir)?;
        let event_frames = events.iter().map(Message::encode_to_vec).collect();
        let persisted = PersistedIdempotentExecution {
            fingerprint: fingerprint.to_string(),
            event_frames,
        };
        let bytes = serde_json::to_vec(&persisted).map_err(invalid_data)?;
        let path = self.path_for_request_id(request_id);
        let tmp = self.tmp_path_for(&path);
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
        self.prune_completed(MAX_IDEMPOTENT_EXECUTIONS)?;
        Ok(())
    }

    fn path_for_request_id(&self, request_id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.json", sanitize_request_id(request_id)))
    }

    fn tmp_path_for(&self, path: &Path) -> PathBuf {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("request.json");
        self.dir.join(format!(".{file_name}.tmp"))
    }

    fn prune_completed(&self, max_entries: usize) -> Result<(), std::io::Error> {
        let mut entries = fs::read_dir(&self.dir)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| {
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((entry.path(), modified))
            })
            .collect::<Vec<_>>();
        if entries.len() <= max_entries {
            return Ok(());
        }
        entries.sort_by_key(|(_, modified)| *modified);
        let remove_count = entries.len().saturating_sub(max_entries);
        for (path, _) in entries.into_iter().take(remove_count) {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
}

fn sanitize_request_id(request_id: &str) -> String {
    let mut safe = request_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .take(160)
        .collect::<String>();
    if safe.is_empty() {
        safe.push_str("request");
    }
    safe
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}

#[derive(Debug)]
struct IdempotentExecution {
    fingerprint: String,
    state: Mutex<IdempotentExecutionState>,
    complete: Condvar,
}

#[derive(Debug)]
struct IdempotentExecutionState {
    events: Vec<proto::ToolEvent>,
    completed: bool,
}

impl IdempotentExecution {
    fn new(fingerprint: String) -> Self {
        Self {
            fingerprint,
            state: Mutex::new(IdempotentExecutionState {
                events: Vec::new(),
                completed: false,
            }),
            complete: Condvar::new(),
        }
    }

    fn record_event(&self, event: proto::ToolEvent) {
        if let Ok(mut state) = self.state.lock() {
            state.events.push(event);
        }
    }

    fn complete(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.completed = true;
            self.complete.notify_all();
        }
    }

    fn is_complete(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.completed)
            .unwrap_or(false)
    }

    fn wait_for_replay(&self) -> Vec<proto::ToolEvent> {
        let mut state = self.state.lock().expect("idempotency cache lock poisoned");
        while !state.completed {
            state = self
                .complete
                .wait(state)
                .expect("idempotency cache lock poisoned");
        }
        state.events.clone()
    }
}

/// Bridge from synchronous `ChunkSink` writes to an mpsc of `ToolEvent`.
struct ChannelChunkSink {
    tx: mpsc::Sender<Result<proto::ToolEvent, Status>>,
}

impl ChannelChunkSink {
    fn new(tx: mpsc::Sender<Result<proto::ToolEvent, Status>>) -> Self {
        Self { tx }
    }

    fn send_chunk(&self, kind: ChunkKind, bytes: &[u8]) {
        let payload = match kind {
            ChunkKind::Stdout => proto::tool_event::Payload::Stdout(proto::StreamChunk {
                data: bytes.to_vec(),
            }),
            ChunkKind::Stderr => proto::tool_event::Payload::Stderr(proto::StreamChunk {
                data: bytes.to_vec(),
            }),
        };
        let _ = self.tx.blocking_send(Ok(proto::ToolEvent {
            payload: Some(payload),
        }));
    }
}

impl ChunkSink for ChannelChunkSink {
    fn stdout(&mut self, chunk: &[u8]) {
        self.send_chunk(ChunkKind::Stdout, chunk);
    }
    fn stderr(&mut self, chunk: &[u8]) {
        self.send_chunk(ChunkKind::Stderr, chunk);
    }
}

/// Streaming sink that forwards chunks and stores the exact event sequence for
/// duplicate idempotency-key requests.
struct RecordingChunkSink {
    inner: ChannelChunkSink,
    execution: Arc<IdempotentExecution>,
}

impl RecordingChunkSink {
    fn new(
        tx: mpsc::Sender<Result<proto::ToolEvent, Status>>,
        execution: Arc<IdempotentExecution>,
    ) -> Self {
        Self {
            inner: ChannelChunkSink::new(tx),
            execution,
        }
    }

    fn send_chunk(&self, kind: ChunkKind, bytes: &[u8]) {
        let payload = match kind {
            ChunkKind::Stdout => proto::tool_event::Payload::Stdout(proto::StreamChunk {
                data: bytes.to_vec(),
            }),
            ChunkKind::Stderr => proto::tool_event::Payload::Stderr(proto::StreamChunk {
                data: bytes.to_vec(),
            }),
        };
        let event = proto::ToolEvent {
            payload: Some(payload),
        };
        self.execution.record_event(event.clone());
        let _ = self.inner.tx.blocking_send(Ok(event));
    }
}

impl ChunkSink for RecordingChunkSink {
    fn stdout(&mut self, chunk: &[u8]) {
        self.send_chunk(ChunkKind::Stdout, chunk);
    }
    fn stderr(&mut self, chunk: &[u8]) {
        self.send_chunk(ChunkKind::Stderr, chunk);
    }
}

/// Bridge from `ChunkSink` to the gRPC `McpToolMessage` bidi stream.
/// Forwards stdout/stderr writes as `StreamChunk` payloads and
/// `ChunkSink::event` JSON values as `event_json` payloads (today the only
/// producer is the connection manager's progress wiring).
struct McpChannelMessageSink {
    tx: mpsc::Sender<Result<proto::McpToolMessage, Status>>,
}

impl McpChannelMessageSink {
    fn new(tx: mpsc::Sender<Result<proto::McpToolMessage, Status>>) -> Self {
        Self { tx }
    }
}

impl ChunkSink for McpChannelMessageSink {
    fn stdout(&mut self, chunk: &[u8]) {
        let _ = self.tx.blocking_send(Ok(proto::McpToolMessage {
            payload: Some(proto::mcp_tool_message::Payload::Stdout(
                proto::StreamChunk {
                    data: chunk.to_vec(),
                },
            )),
        }));
    }
    fn stderr(&mut self, chunk: &[u8]) {
        let _ = self.tx.blocking_send(Ok(proto::McpToolMessage {
            payload: Some(proto::mcp_tool_message::Payload::Stderr(
                proto::StreamChunk {
                    data: chunk.to_vec(),
                },
            )),
        }));
    }
    fn event(&mut self, event: serde_json::Value) {
        let _ = self.tx.blocking_send(Ok(proto::McpToolMessage {
            payload: Some(proto::mcp_tool_message::Payload::EventJson(
                event.to_string(),
            )),
        }));
    }
}

/// Pending-elicitations map keyed by stream-local request id (decimal
/// string). Each in-flight request parks a `oneshot::Sender` here that the
/// inbound loop pops when the matching response arrives.
type PendingElicitations = Arc<Mutex<HashMap<String, oneshot::Sender<ElicitationResponse>>>>;

/// One per-call routing record held by [`BidiElicitationRouter`].
struct BidiRoute {
    outbound: mpsc::Sender<Result<proto::McpToolMessage, Status>>,
    pending: PendingElicitations,
    counter: Arc<std::sync::atomic::AtomicU64>,
}

/// Service-wide registry that maps an MCP server label to the currently
/// active gRPC bidi call routing for that server. See
/// [`ToolRunnerService`] for the rationale.
///
/// Implements [`ElicitationHandler`]; the gRPC server installs an instance
/// at construction time and the same instance is provided to the runner
/// (typically via `LocalToolRunner::with_elicitation_handler(...)`) so
/// MCP server elicitations route to the matching `CallMcpTool` bidi
/// stream.
#[derive(Default)]
pub struct BidiElicitationRouter {
    routes: Mutex<HashMap<String, Vec<Arc<BidiRoute>>>>,
}

/// RAII guard returned by [`BidiElicitationRouter::register`]. Drops the
/// route when the call finishes (or panics) so subsequent elicitations no
/// longer flow to a closed stream.
pub(crate) struct RouteGuard {
    router: Arc<BidiElicitationRouter>,
    server: String,
    route_ptr: *const BidiRoute,
}

// Safety: `route_ptr` is only used as an identity comparison key inside the
// router's mutex; we never deref it.
unsafe impl Send for RouteGuard {}
unsafe impl Sync for RouteGuard {}

impl Drop for RouteGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.router.routes.lock() {
            if let Some(list) = map.get_mut(&self.server) {
                list.retain(|r| (Arc::as_ptr(r) as *const _) != self.route_ptr);
                if list.is_empty() {
                    map.remove(&self.server);
                }
            }
        }
    }
}

impl BidiElicitationRouter {
    fn register(self: &Arc<Self>, server: String, route: Arc<BidiRoute>) -> RouteGuard {
        let key = server.to_ascii_lowercase();
        let route_ptr = Arc::as_ptr(&route) as *const _;
        if let Ok(mut map) = self.routes.lock() {
            map.entry(key.clone()).or_default().push(route);
        }
        RouteGuard {
            router: Arc::clone(self),
            server: key,
            route_ptr,
        }
    }
}

impl std::fmt::Debug for BidiElicitationRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let depth = self
            .routes
            .lock()
            .map(|m| m.values().map(Vec::len).sum::<usize>())
            .unwrap_or(0);
        f.debug_struct("BidiElicitationRouter")
            .field("active_routes", &depth)
            .finish()
    }
}

impl ElicitationHandler for BidiElicitationRouter {
    fn elicit(&self, request: ElicitationRequest) -> ElicitationResponse {
        let key = request.server.to_ascii_lowercase();
        // Pick the most recent registration for this server. Cloning the
        // `Arc` keeps the route alive while we use it even if it gets
        // unregistered concurrently.
        let route = match self.routes.lock() {
            Ok(map) => map.get(&key).and_then(|list| list.last().cloned()),
            Err(_) => None,
        };
        let Some(route) = route else {
            // No active gRPC stream for this server — fall back to the
            // safest default.
            return ElicitationResponse::Decline;
        };
        let request_id = route
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let request_id = request_id.to_string();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut map) = route.pending.lock() {
            map.insert(request_id.clone(), tx);
        }
        let envelope = proto::McpToolMessage {
            payload: Some(proto::mcp_tool_message::Payload::ElicitationRequest(
                proto::McpElicitationRequest {
                    request_id: request_id.clone(),
                    server: request.server,
                    tool: request.tool,
                    message: request.message,
                    mode: match request.mode {
                        ElicitationMode::Form => "form".into(),
                        ElicitationMode::Url => "url".into(),
                    },
                    schema_json: if request.schema.is_null() {
                        String::new()
                    } else {
                        request.schema.to_string()
                    },
                    url: request.url,
                    elicitation_id: request.elicitation_id,
                },
            )),
        };
        if route.outbound.blocking_send(Ok(envelope)).is_err() {
            // Stream closed before we could ship the request.
            if let Ok(mut map) = route.pending.lock() {
                map.remove(&request_id);
            }
            return ElicitationResponse::Decline;
        }
        match rx.blocking_recv() {
            Ok(response) => response,
            Err(_) => {
                // Stream closed mid-flight, or inbound loop dropped the
                // sender. Treat as a decline so the MCP server gets a
                // typed reply rather than hanging.
                ElicitationResponse::Decline
            }
        }
    }
}

fn parse_elicitation_response(resp: &proto::McpElicitationResponse) -> ElicitationResponse {
    match resp.action.as_str() {
        "accept" => {
            let content = if resp.content_json.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(&resp.content_json).unwrap_or(serde_json::Value::Null)
            };
            ElicitationResponse::accept(content)
        }
        "cancel" => ElicitationResponse::Cancel,
        _ => ElicitationResponse::Decline,
    }
}

// FnChunkSink is unused here but kept in the public api for symmetry; silence
// the "unused import" warning when the module compiles standalone.
#[allow(dead_code)]
fn _force_link_fn_chunk_sink<F: FnMut(ChunkKind, &[u8]) + Send>(_: FnChunkSink<F>) {}
