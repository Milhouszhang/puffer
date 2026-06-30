//! Late-bound bridge from workflow-run completion to the dock badge.
//!
//! `subscriptions::install` runs before the daemon's event `Sender` exists
//! (main.rs ordering), so the observer reads this `OnceLock` lazily. The
//! daemon fills it at startup; plain CLI runs never do, so the observer is a
//! silent no-op there.

use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast::Sender;

use puffer_subscriptions::{RunFinishedObserver, WorkflowBindingRun};

use crate::daemon::ServerEnvelope;

static SINK: OnceLock<Sender<ServerEnvelope>> = OnceLock::new();

/// Registers the daemon WS event sender. Idempotent: a second call is ignored.
pub fn set_workflow_run_event_sink(tx: Sender<ServerEnvelope>) {
    let _ = SINK.set(tx);
}

/// Emits a non-replay `workflow-run:finished` event for one finished run.
/// No-op when no sink is registered or there are no receivers (best-effort).
pub fn emit_workflow_run_finished(run: &WorkflowBindingRun) {
    if let Some(tx) = SINK.get() {
        let _ = tx.send(ServerEnvelope::Event {
            event: "workflow-run:finished".to_string(),
            payload: serde_json::json!({
                "type": "workflow-run-finished",
                "slug": run.workflow_slug,
                "runId": run.run_id,
                "status": run.status,
            }),
        });
    }
}

/// The observer to attach to the subscription manager's history store.
pub fn workflow_run_finished_observer() -> RunFinishedObserver {
    Arc::new(|run: &WorkflowBindingRun| emit_workflow_run_finished(run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_subscriptions::{WorkflowBindingRun, WorkflowBindingRunStatus};
    use serde_json::json;

    fn run(status: WorkflowBindingRunStatus) -> WorkflowBindingRun {
        WorkflowBindingRun {
            idx: 1,
            run_id: "run-xyz".into(),
            workflow_slug: "demo".into(),
            trigger_info: json!({}),
            action_summary: json!({}),
            action_log: vec![],
            status,
            started_at_ms: 0,
            ended_at_ms: 0,
        }
    }

    #[tokio::test]
    async fn emits_finished_envelope_on_the_workflow_run_channel() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(8);
        set_workflow_run_event_sink(tx);
        emit_workflow_run_finished(&run(WorkflowBindingRunStatus::Completed));

        let env = rx.recv().await.unwrap();
        match env {
            crate::daemon::ServerEnvelope::Event { event, payload } => {
                assert_eq!(event, "workflow-run:finished");
                assert_eq!(payload["type"], "workflow-run-finished");
                assert_eq!(payload["slug"], "demo");
                assert_eq!(payload["runId"], "run-xyz");
                assert_eq!(payload["status"], "completed");
            }
            other => panic!("unexpected envelope: {other:?}"),
        }
    }
}
