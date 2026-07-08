//! Background task system combining Claude Code and Codex best practices.
//!
//! Key features:
//! - **HeadTailBuffer** (Codex): Efficient output capture preserving first and last
//!   portions of large outputs while discarding the middle.
//! - **BackgroundTaskManager** (CC + Codex): Centralized, thread-safe task registry
//!   with concurrent task limits and completion notifications.
//! - **Auto-backgrounding** (CC): Long-running tasks automatically move to background
//!   after a configurable timeout budget.

use crate::runtime::CancelToken;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// HeadTailBuffer — Codex-style output capture
// ---------------------------------------------------------------------------

/// Maximum total buffer size: 1 MiB.
const HEAD_TAIL_TOTAL_BYTES: usize = 1024 * 1024;

/// Head portion: first 512 KB.
const HEAD_CAPACITY: usize = HEAD_TAIL_TOTAL_BYTES / 2;

/// Tail portion: last 512 KB.
const TAIL_CAPACITY: usize = HEAD_TAIL_TOTAL_BYTES / 2;

/// Captures the beginning and end of a potentially large output stream,
/// discarding the middle when the total exceeds `HEAD_CAPACITY + TAIL_CAPACITY`.
///
/// This mirrors Codex's Head-Tail Buffer strategy: preserve setup/error output
/// (head) and final results/summary (tail), which are typically the most
/// informative portions.
#[derive(Debug, Clone)]
pub struct HeadTailBuffer {
    head: Vec<u8>,
    tail: RingBuffer,
    total_written: usize,
    head_full: bool,
    head_capacity: usize,
    tail_capacity: usize,
}

impl HeadTailBuffer {
    /// Creates a new empty buffer with default 1 MiB capacity (512KB head + 512KB tail).
    pub fn new() -> Self {
        Self::with_capacity(HEAD_CAPACITY, TAIL_CAPACITY)
    }

    /// Creates a buffer with custom head and tail capacities.
    pub fn with_capacity(head_cap: usize, tail_cap: usize) -> Self {
        Self {
            head: Vec::with_capacity(head_cap.min(64 * 1024)), // lazy alloc
            tail: RingBuffer::new(tail_cap),
            total_written: 0,
            head_full: false,
            head_capacity: head_cap,
            tail_capacity: tail_cap,
        }
    }

    /// Appends data to the buffer. Once the head is full, subsequent data
    /// goes into the tail ring buffer (overwriting oldest tail data).
    pub fn write(&mut self, data: &[u8]) {
        self.total_written += data.len();

        if !self.head_full {
            let head_remaining = self.head_capacity.saturating_sub(self.head.len());
            if data.len() <= head_remaining {
                self.head.extend_from_slice(data);
                return;
            }
            // Fill head, rest goes to tail.
            self.head.extend_from_slice(&data[..head_remaining]);
            self.head_full = true;
            self.tail.write(&data[head_remaining..]);
        } else {
            self.tail.write(data);
        }
    }

    /// Appends a string to the buffer.
    pub fn write_str(&mut self, s: &str) {
        self.write(s.as_bytes());
    }

    /// Returns the total number of bytes written (including discarded middle).
    pub fn total_written(&self) -> usize {
        self.total_written
    }

    /// Returns true if the middle section was truncated.
    pub fn was_truncated(&self) -> bool {
        self.total_written > self.head_capacity + self.tail_capacity
    }

    /// Returns the number of bytes that were discarded from the middle.
    pub fn bytes_dropped(&self) -> usize {
        if !self.was_truncated() {
            return 0;
        }
        self.total_written - self.head.len() - self.tail.len()
    }

    /// Materializes the buffer contents as a string, inserting a truncation
    /// marker between head and tail when data was dropped.
    pub fn to_string_lossy(&self) -> String {
        if !self.was_truncated() {
            // No truncation — head contains everything, tail may have overflow
            // that wasn't needed.
            let mut result = String::from_utf8_lossy(&self.head).into_owned();
            let tail_bytes = self.tail.to_vec();
            if !tail_bytes.is_empty() {
                result.push_str(&String::from_utf8_lossy(&tail_bytes));
            }
            return result;
        }

        let dropped = self.bytes_dropped();
        let head_str = String::from_utf8_lossy(&self.head);
        let tail_bytes = self.tail.to_vec();
        let tail_str = String::from_utf8_lossy(&tail_bytes);

        format!("{head_str}\n\n[... {dropped} bytes truncated ...]\n\n{tail_str}")
    }
}

impl Default for HeadTailBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple ring buffer for the tail portion.
#[derive(Debug, Clone)]
struct RingBuffer {
    buf: Vec<u8>,
    capacity: usize,
    write_pos: usize,
    len: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity],
            capacity,
            write_pos: 0,
            len: 0,
        }
    }

    fn write(&mut self, data: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        // If data is larger than capacity, only keep the last `capacity` bytes.
        let effective = if data.len() > self.capacity {
            &data[data.len() - self.capacity..]
        } else {
            data
        };

        for &byte in effective {
            self.buf[self.write_pos] = byte;
            self.write_pos = (self.write_pos + 1) % self.capacity;
            if self.len < self.capacity {
                self.len += 1;
            }
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn to_vec(&self) -> Vec<u8> {
        if self.len == 0 {
            return Vec::new();
        }
        if self.len < self.capacity {
            // Buffer hasn't wrapped yet.
            self.buf[..self.len].to_vec()
        } else {
            // Buffer has wrapped: read_pos is at write_pos.
            let mut result = Vec::with_capacity(self.capacity);
            result.extend_from_slice(&self.buf[self.write_pos..]);
            result.extend_from_slice(&self.buf[..self.write_pos]);
            result
        }
    }
}

// ---------------------------------------------------------------------------
// BackgroundTaskManager — centralized task registry
// ---------------------------------------------------------------------------

/// Maximum number of concurrently running background tasks (from Codex: 16 workers).
const MAX_CONCURRENT_TASKS: usize = 16;

/// Default auto-backgrounding timeout budget (from CC: 15 seconds).
pub const AUTO_BACKGROUND_BUDGET: Duration = Duration::from_secs(15);

/// Lifecycle state of a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskStatus {
    /// Task is queued but has not started executing yet.
    Pending,
    /// Task is currently running.
    Running,
    /// A stop was requested; the worker has not yet acknowledged.
    Stopping,
    /// Task completed successfully.
    Completed,
    /// Task failed with an error.
    Failed,
    /// Task was cancelled/stopped by the user or system.
    Stopped,
    /// Task was scoped to a turn that finished before the worker acked its stop.
    Abandoned,
}

impl BackgroundTaskStatus {
    /// Returns true if the task has reached a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Stopped | Self::Abandoned
        )
    }
}

/// Whether a background task hosts an agent loop or a shell process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskKind {
    Agent,
    Shell,
}

/// Whether a background task is bound to its owner turn or detached from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskDurability {
    /// Stopped when the owner turn finishes.
    ScopedToTurn,
    /// Survives the owner turn; only an explicit `TaskStop` ends it.
    DurableAcrossTurns,
}

impl BackgroundTaskDurability {
    /// Maps the tool-facing `durable` boolean to a durability.
    pub fn from_durable_flag(durable: bool) -> Self {
        if durable {
            Self::DurableAcrossTurns
        } else {
            Self::ScopedToTurn
        }
    }
}

/// Full registration record for an owner-aware, stoppable background task.
pub struct BackgroundTaskRegistration {
    pub task_id: String,
    pub description: String,
    pub kind: BackgroundTaskKind,
    pub agent_id: Option<String>,
    pub output_file: Option<String>,
    pub auto_backgrounded: bool,
    pub owner_turn_id: Option<String>,
    pub owner_session_id: Option<String>,
    pub durability: BackgroundTaskDurability,
    pub cancel: Option<CancelToken>,
    pub process_id: Option<u32>,
}

/// Summary of what `stop_scoped_by_turn` did for one turn.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TurnTaskStopReport {
    pub stop_requested: usize,
    pub abandoned: usize,
}

/// Metadata and state for one background task.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundTaskInfo {
    pub task_id: String,
    pub description: String,
    pub status: BackgroundTaskStatus,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
    /// True if this task was auto-backgrounded (CC-style timeout).
    pub auto_backgrounded: bool,
    /// Whether this task hosts an agent loop or a shell process.
    pub kind: BackgroundTaskKind,
    /// Turn that spawned this task, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_turn_id: Option<String>,
    /// Session that spawned this task, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_id: Option<String>,
    /// Whether this task is scoped to its owner turn or detached.
    pub durability: BackgroundTaskDurability,
    /// OS pid of the underlying process, for shell tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
}

/// Shared mutable state for one tracked background task.
struct TrackedTask {
    info: BackgroundTaskInfo,
    output: Arc<Mutex<HeadTailBuffer>>,
    cancel: Option<CancelToken>,
    _start_instant: Instant,
}

/// Thread-safe centralized manager for all background tasks.
///
/// Combines CC's task notification model with Codex's concurrent worker limits.
pub struct BackgroundTaskManager {
    tasks: Mutex<HashMap<String, TrackedTask>>,
}

impl BackgroundTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a new owner-aware background task. Returns `Err` if the
    /// concurrent limit has been reached.
    pub fn register_with_options(
        &self,
        registration: BackgroundTaskRegistration,
    ) -> Result<Arc<Mutex<HeadTailBuffer>>, String> {
        let mut tasks = self.tasks.lock().unwrap();

        // Enforce concurrent limit (Codex: 16 workers).
        let active_count = tasks
            .values()
            .filter(|t| !t.info.status.is_terminal())
            .count();
        if active_count >= MAX_CONCURRENT_TASKS {
            return Err(format!(
                "concurrent background task limit reached ({MAX_CONCURRENT_TASKS}). \
                 Wait for existing tasks to complete before launching new ones."
            ));
        }

        let output = Arc::new(Mutex::new(HeadTailBuffer::new()));
        let info = BackgroundTaskInfo {
            task_id: registration.task_id.clone(),
            description: registration.description,
            status: BackgroundTaskStatus::Running,
            created_at: now_ms(),
            completed_at: None,
            agent_id: registration.agent_id,
            output_file: registration.output_file,
            auto_backgrounded: registration.auto_backgrounded,
            kind: registration.kind,
            owner_turn_id: registration.owner_turn_id,
            owner_session_id: registration.owner_session_id,
            durability: registration.durability,
            process_id: registration.process_id,
        };
        tasks.insert(
            registration.task_id,
            TrackedTask {
                info,
                output: Arc::clone(&output),
                cancel: registration.cancel,
                _start_instant: Instant::now(),
            },
        );
        Ok(output)
    }

    /// Registers a new background task. Returns `Err` if the concurrent limit
    /// has been reached. Legacy no-owner callers default to a durable agent so
    /// nothing new can stop them.
    pub fn register(
        &self,
        task_id: &str,
        description: &str,
        agent_id: Option<&str>,
        output_file: Option<&str>,
        auto_backgrounded: bool,
    ) -> Result<Arc<Mutex<HeadTailBuffer>>, String> {
        self.register_with_options(BackgroundTaskRegistration {
            task_id: task_id.to_string(),
            description: description.to_string(),
            kind: BackgroundTaskKind::Agent,
            agent_id: agent_id.map(str::to_string),
            output_file: output_file.map(str::to_string),
            auto_backgrounded,
            owner_turn_id: None,
            owner_session_id: None,
            durability: BackgroundTaskDurability::DurableAcrossTurns,
            cancel: None,
            process_id: None,
        })
    }

    /// Marks a task as completed or failed. Terminal statuses are monotonic
    /// (a late duplicate never overwrites), and a worker acking a requested
    /// stop lands in `Stopped` rather than `Completed`/`Failed`.
    pub fn complete(&self, task_id: &str, success: bool) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            if task.info.status.is_terminal() {
                return;
            }
            task.info.status = if task.info.status == BackgroundTaskStatus::Stopping {
                BackgroundTaskStatus::Stopped
            } else if success {
                BackgroundTaskStatus::Completed
            } else {
                BackgroundTaskStatus::Failed
            };
            task.info.completed_at = Some(now_ms());
        }
    }

    /// Requests a stop of one task: cancels its token, kills its process if
    /// any, and marks it `Stopping`. Returns its info snapshot, or `None` if the
    /// task is unknown. A no-op on already-terminal tasks.
    pub fn request_stop(&self, task_id: &str) -> Option<BackgroundTaskInfo> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks.get_mut(task_id)?;
        if !task.info.status.is_terminal() {
            stop_one(task, "task_stop");
        }
        Some(task.info.clone())
    }

    /// Stops every live task scoped to `turn_id`, then waits up to
    /// `drain_timeout` for the workers to ack. Any still-live scoped task at the
    /// deadline is force-marked `Abandoned`. Durable tasks are never touched.
    pub fn stop_scoped_by_turn(
        &self,
        turn_id: &str,
        reason: &str,
        drain_timeout: Duration,
    ) -> TurnTaskStopReport {
        let mut report = TurnTaskStopReport::default();
        let is_scoped_live = |info: &BackgroundTaskInfo| {
            info.owner_turn_id.as_deref() == Some(turn_id)
                && info.durability == BackgroundTaskDurability::ScopedToTurn
                && !info.status.is_terminal()
        };
        {
            let mut tasks = self.tasks.lock().unwrap();
            for task in tasks.values_mut() {
                if is_scoped_live(&task.info) {
                    stop_one(task, reason);
                    report.stop_requested += 1;
                }
            }
        }
        let start = Instant::now();
        while start.elapsed() < drain_timeout {
            let any_live = self
                .tasks
                .lock()
                .unwrap()
                .values()
                .any(|t| is_scoped_live(&t.info));
            if !any_live {
                return report;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut tasks = self.tasks.lock().unwrap();
        for task in tasks.values_mut() {
            if is_scoped_live(&task.info) {
                task.info.status = BackgroundTaskStatus::Abandoned;
                task.info.completed_at = Some(now_ms());
                report.abandoned += 1;
            }
        }
        report
    }

    /// Marks a task as stopped (cancelled).
    pub fn stop(&self, task_id: &str) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.info.status = BackgroundTaskStatus::Stopped;
            task.info.completed_at = Some(now_ms());
        }
    }

    /// Returns a snapshot of the task info, if it exists.
    pub fn get_info(&self, task_id: &str) -> Option<BackgroundTaskInfo> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(task_id).map(|t| t.info.clone())
    }

    /// Returns the current output buffer contents for a task.
    pub fn read_output(&self, task_id: &str) -> Option<String> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(task_id).map(|t| {
            let buf = t.output.lock().unwrap();
            buf.to_string_lossy()
        })
    }

    /// Lists all non-terminal (active) tasks.
    pub fn active_tasks(&self) -> Vec<BackgroundTaskInfo> {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|t| !t.info.status.is_terminal())
            .map(|t| t.info.clone())
            .collect()
    }

    /// Lists all tasks (including completed).
    pub fn all_tasks(&self) -> Vec<BackgroundTaskInfo> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().map(|t| t.info.clone()).collect()
    }

    /// Returns the number of currently active (non-terminal) tasks.
    pub fn active_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|t| !t.info.status.is_terminal())
            .count()
    }

    /// Removes completed tasks older than the given duration to prevent unbounded growth.
    pub fn cleanup_older_than(&self, max_age: Duration) {
        let mut tasks = self.tasks.lock().unwrap();
        let cutoff = now_ms().saturating_sub(max_age.as_millis() as u64);
        tasks.retain(|_, t| {
            // Keep all non-terminal tasks and recently completed ones.
            !t.info.status.is_terminal() || t.info.completed_at.unwrap_or(u64::MAX) > cutoff
        });
    }

    /// Checks whether a new task can be registered without exceeding the limit.
    pub fn has_capacity(&self) -> bool {
        self.active_count() < MAX_CONCURRENT_TASKS
    }

    /// Block until every active task has reached a terminal status, or
    /// the timeout elapses. Used at process-exit so detached
    /// background-agent threads (`launch_background_agent`) get a
    /// chance to finish their LLM round-trip and flush OTel spans
    /// before `shutdown_runtime_services` closes the exporter.
    /// Returns the number of tasks still active when the wait ended.
    pub fn wait_for_drain(&self, timeout: Duration) -> usize {
        let start = std::time::Instant::now();
        loop {
            let active = self.active_count();
            if active == 0 {
                return 0;
            }
            if start.elapsed() >= timeout {
                return active;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Global singleton for the background task manager.
static MANAGER: OnceLock<BackgroundTaskManager> = OnceLock::new();

/// Returns the global background task manager, initializing on first access.
pub fn task_manager() -> &'static BackgroundTaskManager {
    MANAGER.get_or_init(BackgroundTaskManager::new)
}

// ---------------------------------------------------------------------------
// Auto-backgrounding support (CC-style)
// ---------------------------------------------------------------------------

/// Wraps a blocking operation with an auto-backgrounding timeout.
///
/// If the operation completes within `budget`, returns `AutoBgResult::Inline(T)`.
/// If it exceeds the budget, the operation continues on its spawned thread and
/// this function returns `AutoBgResult::Backgrounded` with a task handle.
///
/// This mirrors Claude Code's assistant-mode behavior where tasks running longer
/// than `ASSISTANT_BLOCKING_BUDGET_MS` (15s) are automatically moved to background.
pub fn run_with_auto_background<F, T>(
    description: &str,
    budget: Duration,
    func: F,
) -> AutoBgResult<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let task_id = format!("auto-bg-{}", Uuid::new_v4().simple());
    let (tx, rx) = std::sync::mpsc::channel();

    let task_id_clone = task_id.clone();
    let description_clone = description.to_string();

    std::thread::spawn(move || {
        let result = func();
        // Try to send the result back. If the receiver is gone (timed out),
        // the result is lost but the task was already registered.
        let _ = tx.send(result);
        // Mark task as completed if it was registered (auto-backgrounded).
        task_manager().complete(&task_id_clone, true);
        eprintln!(
            "[background-tasks] auto-backgrounded task completed: {task_id_clone} ({description_clone})"
        );
    });

    // Wait up to `budget` for the result.
    match rx.recv_timeout(budget) {
        Ok(result) => AutoBgResult::Inline(result),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // Register the task in the manager since it's now background.
            let _ = task_manager().register(
                &task_id,
                description,
                None,
                None,
                true, // auto_backgrounded
            );
            eprintln!(
                "[background-tasks] task exceeded {}ms budget, auto-backgrounded: {description} (id: {task_id})",
                budget.as_millis()
            );
            AutoBgResult::Backgrounded {
                task_id,
                description: description.to_string(),
            }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            // Thread panicked or channel broken.
            AutoBgResult::Backgrounded {
                task_id,
                description: description.to_string(),
            }
        }
    }
}

/// Result of an auto-backgrounding operation.
#[derive(Debug)]
pub enum AutoBgResult<T> {
    /// The operation completed within the budget.
    Inline(T),
    /// The operation was auto-backgrounded because it exceeded the budget.
    Backgrounded {
        task_id: String,
        description: String,
    },
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Cancels one task's token, kills its process if any, notes the stop in its
/// output, and marks it `Stopping`.
fn stop_one(task: &mut TrackedTask, reason: &str) {
    if let Some(cancel) = &task.cancel {
        cancel.cancel();
    }
    if let Some(pid) = task.info.process_id {
        let _ = crate::runtime::claude_tools::workflow::store::terminate_process(pid);
    }
    if let Ok(mut output) = task.output.lock() {
        output.write_str(&format!("\n[stop requested: {reason}]\n"));
    }
    task.info.status = BackgroundTaskStatus::Stopping;
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Formats a byte count into a human-readable string.
pub fn format_buffer_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
#[path = "background_tasks_tests.rs"]
mod tests;
