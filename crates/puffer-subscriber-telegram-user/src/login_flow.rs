//! Pure decision layer for the Telegram login state machine.
//!
//! Zero I/O. Every function maps (current phase, classified effect result)
//! to (next phase, control events). The effect layer in `login.rs` /
//! `qr_login.rs` owns the grammers `Client`, runs bounded network calls,
//! classifies failures into [`ErrClass`], and applies the returned
//! [`Decision`]. Phases are generic over token types because grammers'
//! `LoginToken` cannot be constructed outside grammers — tests drive the
//! full transition table with dummy tokens.

use serde_json::{json, Value};

/// Login flow phase. `C` = login-code token, `P` = 2FA password token.
pub enum LoginPhase<C, P> {
    Idle,
    CodeSent {
        token: C,
        requested_at_ms: u64,
    },
    PasswordPending {
        token: P,
        hint: Option<String>,
    },
    QrPending {
        dc_id: i32,
        expires_at: i32,
        refreshes: u8,
    },
    Authorized,
}

impl<C, P> LoginPhase<C, P> {
    pub fn name(&self) -> &'static str {
        match self {
            LoginPhase::Idle => "idle",
            LoginPhase::CodeSent { .. } => "code_sent",
            LoginPhase::PasswordPending { .. } => "password_pending",
            LoginPhase::QrPending { .. } => "qr_pending",
            LoginPhase::Authorized => "authorized",
        }
    }

    pub fn is_authorized(&self) -> bool {
        matches!(self, LoginPhase::Authorized)
    }
}

/// Classified failure of one bounded network effect.
pub enum ErrClass<P> {
    /// Telegram rejected the login code (`PHONE_CODE_*`; grammers folds
    /// expired and mistyped codes into a single error).
    InvalidCode,
    /// Telegram rejected the 2FA password (`PASSWORD_HASH_INVALID`).
    InvalidPassword,
    /// Code/QR accepted; the account requires its 2FA cloud password.
    PasswordRequired(P),
    /// Telegram asked for a fresh auth session (`AUTH_RESTART`).
    AuthRestart,
    /// Transient transport failure; retrying on a fresh connection may work.
    Transport(String),
    /// The bounded call hit `LOGIN_NETWORK_TIMEOUT`.
    Timeout,
    /// Anything else; text passes through verbatim (incl. FLOOD_WAIT).
    Fatal(String),
}

/// Successful authorization, reduced to plain data.
pub struct AuthorizedUser {
    pub id: i64,
    pub first_name: Option<String>,
}

/// One control event to emit on the subscriber topic.
pub struct ControlEventSpec {
    pub kind: &'static str,
    pub payload: Value,
}

/// Next phase + events to emit. When `next` is `Authorized`, the applier
/// MUST save+promote the session BEFORE emitting the events (#551).
pub struct Decision<C, P> {
    pub next: LoginPhase<C, P>,
    pub events: Vec<ControlEventSpec>,
}

/// Uniform `login_error` builder: the only way this module produces login
/// errors, so `phase`/`reason`/`retryable`/`error` are always present.
pub fn login_error_event(
    phase: &'static str,
    reason: &'static str,
    retryable: bool,
    error: String,
) -> ControlEventSpec {
    ControlEventSpec {
        kind: "login_error",
        payload: json!({
            "phase": phase,
            "reason": reason,
            "retryable": retryable,
            "error": error,
        }),
    }
}

/// A submit command arrived in a phase that cannot handle it. Never mutates
/// state; tells the operator how to restart.
pub fn wrong_phase_error(command: &'static str, phase_name: &'static str) -> ControlEventSpec {
    let mut event = login_error_event(
        "protocol",
        "unexpected_command",
        false,
        format!("{command} not expected in login phase `{phase_name}`; run login_start (or login_qr) first"),
    );
    event.payload["recovery"] = json!("restart_login");
    event
}

/// Classifies an error's display text. Callers that can match typed grammers
/// variants (`SignInError`) do so first and only fall back to this.
pub fn classify_error_text<P>(text: String) -> ErrClass<P> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("auth_restart") {
        return ErrClass::AuthRestart;
    }
    let transport = [
        "read 0 bytes",
        "connection reset",
        "connection aborted",
        "broken pipe",
        "unexpected eof",
    ];
    if transport.iter().any(|needle| lower.contains(needle)) {
        return ErrClass::Transport(text);
    }
    ErrClass::Fatal(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Migrated from login.rs::tests (those get deleted in Task 3).
    #[test]
    fn transport_errors_classify_as_transport() {
        for text in [
            "request error: read error, IO failed: read 0 bytes",
            "request error: read error, IO failed: connection reset by peer",
            "broken pipe",
            "unexpected eof",
            "connection aborted",
        ] {
            assert!(matches!(
                classify_error_text::<u8>(text.to_string()),
                ErrClass::Transport(_)
            ));
        }
    }

    #[test]
    fn auth_restart_classifies_as_auth_restart() {
        assert!(matches!(
            classify_error_text::<u8>(
                "request error: rpc error 500: AUTH_RESTART caused by auth.sendCode".into()
            ),
            ErrClass::AuthRestart
        ));
    }

    #[test]
    fn unknown_errors_classify_as_fatal_with_text_passthrough() {
        let ErrClass::Fatal(text) = classify_error_text::<u8>("PHONE_NUMBER_INVALID".into()) else {
            panic!("expected Fatal")
        };
        assert_eq!(text, "PHONE_NUMBER_INVALID");
    }

    #[test]
    fn login_error_event_always_carries_contract_fields() {
        let event = login_error_event(
            "sign_in",
            "invalid_or_expired_code",
            true,
            "invalid code".into(),
        );
        assert_eq!(event.kind, "login_error");
        assert_eq!(event.payload["phase"], "sign_in");
        assert_eq!(event.payload["reason"], "invalid_or_expired_code");
        assert_eq!(event.payload["retryable"], true);
        assert_eq!(event.payload["error"], "invalid code");
    }

    #[test]
    fn wrong_phase_error_does_not_look_retryable() {
        let event = wrong_phase_error("submit_password", "idle");
        assert_eq!(event.payload["reason"], "unexpected_command");
        assert_eq!(event.payload["retryable"], false);
        assert_eq!(event.payload["recovery"], "restart_login");
        assert!(event.payload["error"]
            .as_str()
            .unwrap()
            .contains("submit_password"));
    }
}
