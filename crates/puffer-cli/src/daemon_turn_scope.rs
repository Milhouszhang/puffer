use puffer_core::{CancelToken, PermissionPromptAction, UserQuestionPromptResponse};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnFinishReason {
    Complete,
    CancelledByUser,
    Error,
    ClientDisconnected,
}

impl TurnFinishReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::CancelledByUser => "cancelled_by_user",
            Self::Error => "error",
            Self::ClientDisconnected => "client_disconnected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolveInteractionError {
    Finished,
    Expired,
    Unknown,
    WorkerReleased,
}

#[derive(Debug)]
pub(crate) enum PendingWait<T> {
    Resolved(T),
    TimedOut,
    Cancelled,
    Released,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TurnFinishReport {
    pub pending_permissions_resolved: usize,
    pub pending_questions_resolved: usize,
}

pub(crate) struct TurnScope {
    cancel: CancelToken,
    interaction_timeout: Duration,
    pending_permissions: Mutex<HashMap<String, mpsc::Sender<PermissionPromptAction>>>,
    pending_questions: Mutex<HashMap<String, mpsc::Sender<UserQuestionPromptResponse>>>,
    expired_requests: Mutex<HashSet<String>>,
    finished: Mutex<bool>,
}

impl TurnScope {
    pub(crate) fn new(cancel: CancelToken, interaction_timeout: Duration) -> Self {
        Self {
            cancel,
            interaction_timeout,
            pending_permissions: Mutex::new(HashMap::new()),
            pending_questions: Mutex::new(HashMap::new()),
            expired_requests: Mutex::new(HashSet::new()),
            finished: Mutex::new(false),
        }
    }

    pub(crate) fn register_permission(
        &self,
        request_id: String,
    ) -> mpsc::Receiver<PermissionPromptAction> {
        let (tx, rx) = mpsc::channel();
        self.pending_permissions
            .lock()
            .unwrap()
            .insert(request_id, tx);
        rx
    }

    pub(crate) fn register_user_question(
        &self,
        request_id: String,
    ) -> mpsc::Receiver<UserQuestionPromptResponse> {
        let (tx, rx) = mpsc::channel();
        self.pending_questions
            .lock()
            .unwrap()
            .insert(request_id, tx);
        rx
    }

    /// Drops a pending user-question responder without resolving it. Used by
    /// long-poll setup helpers (WeChat) that re-issue or abandon a prompt from
    /// their own `recv_timeout` loop instead of a single bounded wait.
    pub(crate) fn deregister_user_question(&self, request_id: &str) {
        self.pending_questions.lock().unwrap().remove(request_id);
    }

    fn check_resolvable(&self, request_id: &str) -> Result<(), ResolveInteractionError> {
        if *self.finished.lock().unwrap() {
            return Err(ResolveInteractionError::Finished);
        }
        if self.expired_requests.lock().unwrap().contains(request_id) {
            return Err(ResolveInteractionError::Expired);
        }
        Ok(())
    }

    pub(crate) fn resolve_permission(
        &self,
        request_id: &str,
        action: PermissionPromptAction,
    ) -> Result<(), ResolveInteractionError> {
        self.check_resolvable(request_id)?;
        let sender = self
            .pending_permissions
            .lock()
            .unwrap()
            .remove(request_id)
            .ok_or(ResolveInteractionError::Unknown)?;
        sender
            .send(action)
            .map_err(|_| ResolveInteractionError::WorkerReleased)
    }

    pub(crate) fn resolve_user_question(
        &self,
        request_id: &str,
        response: UserQuestionPromptResponse,
    ) -> Result<(), ResolveInteractionError> {
        self.check_resolvable(request_id)?;
        let sender = self
            .pending_questions
            .lock()
            .unwrap()
            .remove(request_id)
            .ok_or(ResolveInteractionError::Unknown)?;
        sender
            .send(response)
            .map_err(|_| ResolveInteractionError::WorkerReleased)
    }

    fn wait_generic<T>(
        &self,
        request_id: &str,
        rx: mpsc::Receiver<T>,
        pending: &Mutex<HashMap<String, mpsc::Sender<T>>>,
    ) -> PendingWait<T> {
        match rx.recv_timeout(self.interaction_timeout) {
            Ok(value) => PendingWait::Resolved(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                pending.lock().unwrap().remove(request_id);
                self.expired_requests
                    .lock()
                    .unwrap()
                    .insert(request_id.to_string());
                if self.cancel.is_cancelled() {
                    PendingWait::Cancelled
                } else {
                    PendingWait::TimedOut
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => PendingWait::Released,
        }
    }

    pub(crate) fn wait_permission(
        &self,
        request_id: &str,
        rx: mpsc::Receiver<PermissionPromptAction>,
    ) -> PendingWait<PermissionPromptAction> {
        self.wait_generic(request_id, rx, &self.pending_permissions)
    }

    pub(crate) fn wait_user_question(
        &self,
        request_id: &str,
        rx: mpsc::Receiver<UserQuestionPromptResponse>,
    ) -> PendingWait<UserQuestionPromptResponse> {
        self.wait_generic(request_id, rx, &self.pending_questions)
    }

    pub(crate) fn finish(&self, reason: TurnFinishReason) -> TurnFinishReport {
        *self.finished.lock().unwrap() = true;
        if !matches!(reason, TurnFinishReason::Complete) {
            self.cancel.cancel();
        }
        let mut report = TurnFinishReport::default();
        for (_, tx) in self.pending_permissions.lock().unwrap().drain() {
            let _ = tx.send(PermissionPromptAction::Deny);
            report.pending_permissions_resolved += 1;
        }
        for (_, tx) in self.pending_questions.lock().unwrap().drain() {
            let mut annotations = Map::new();
            annotations.insert(
                "_puffer_finish_reason".to_string(),
                Value::String(reason.as_str().to_string()),
            );
            let _ = tx.send(UserQuestionPromptResponse {
                answers: Map::new(),
                annotations,
            });
            report.pending_questions_resolved += 1;
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_core::{CancelToken, PermissionPromptAction};
    use std::time::Duration;

    #[test]
    fn permission_wait_times_out_and_late_resolve_is_expired() {
        let scope = TurnScope::new(CancelToken::new(), Duration::from_millis(5));
        let rx = scope.register_permission("req-1".to_string());

        let waited = scope.wait_permission("req-1", rx);
        assert!(matches!(waited, PendingWait::TimedOut));

        let err = scope
            .resolve_permission("req-1", PermissionPromptAction::AllowOnce)
            .unwrap_err();
        assert_eq!(err, ResolveInteractionError::Expired);
    }

    #[test]
    fn finish_denies_pending_permissions_and_blocks_late_resolves() {
        let scope = TurnScope::new(CancelToken::new(), Duration::from_secs(60));
        let rx = scope.register_permission("req-1".to_string());

        let report = scope.finish(TurnFinishReason::CancelledByUser);
        assert_eq!(report.pending_permissions_resolved, 1);
        assert_eq!(rx.recv().expect("denied"), PermissionPromptAction::Deny);

        let err = scope
            .resolve_permission("req-1", PermissionPromptAction::AllowOnce)
            .unwrap_err();
        assert_eq!(err, ResolveInteractionError::Finished);
    }

    #[test]
    fn resolve_then_wait_delivers_and_duplicate_is_unknown() {
        let scope = TurnScope::new(CancelToken::new(), Duration::from_secs(60));
        let rx = scope.register_permission("req-1".to_string());
        scope
            .resolve_permission("req-1", PermissionPromptAction::AllowSession)
            .unwrap();
        assert!(matches!(
            scope.wait_permission("req-1", rx),
            PendingWait::Resolved(PermissionPromptAction::AllowSession)
        ));
        assert_eq!(
            scope
                .resolve_permission("req-1", PermissionPromptAction::Deny)
                .unwrap_err(),
            ResolveInteractionError::Unknown
        );
    }
}
