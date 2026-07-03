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

pub fn awaiting_code_event(phone: &str) -> ControlEventSpec {
    ControlEventSpec {
        kind: "login_awaiting_code",
        payload: json!({ "phone": phone }),
    }
}

pub fn awaiting_password_event(
    phone: Option<&str>,
    hint: Option<&str>,
    qr_login: bool,
) -> ControlEventSpec {
    let mut payload = json!({});
    if let Some(phone) = phone {
        payload["phone"] = json!(phone);
    }
    if let Some(hint) = hint {
        payload["password_hint"] = json!(hint);
    }
    if qr_login {
        payload["qr_login"] = json!(true);
    }
    ControlEventSpec {
        kind: "login_awaiting_password",
        payload,
    }
}

pub fn login_complete_event(user: &AuthorizedUser, qr_login: bool) -> ControlEventSpec {
    let mut payload = json!({ "user_id": user.id, "first_name": user.first_name });
    if qr_login {
        payload["qr_login"] = json!(true);
    }
    ControlEventSpec {
        kind: "login_complete",
        payload,
    }
}

pub enum StartFailOp {
    Connect,
    RequestCode,
}

pub enum StartStep<C, P> {
    Decided(Decision<C, P>),
    /// AUTH_RESTART on the first attempt: reconnect with a fresh session and retry.
    RetryFreshSession,
}

pub fn decide_start<C, P>(
    phone: &str,
    result: Result<C, (StartFailOp, ErrClass<P>)>,
    attempt: u8,
    now_ms: u64,
) -> StartStep<C, P> {
    match result {
        Ok(token) => StartStep::Decided(Decision {
            next: LoginPhase::CodeSent {
                token,
                requested_at_ms: now_ms,
            },
            events: vec![awaiting_code_event(phone)],
        }),
        Err((_, ErrClass::AuthRestart)) if attempt == 0 => StartStep::RetryFreshSession,
        Err((op, err)) => {
            let phase = match op {
                StartFailOp::Connect => "connect",
                StartFailOp::RequestCode => "request_code",
            };
            let (reason, retryable, text) = describe(err);
            StartStep::Decided(Decision {
                next: LoginPhase::Idle,
                events: vec![login_error_event(phase, reason, retryable, text)],
            })
        }
    }
}

pub fn decide_code_submit<C, P>(
    token: C,
    requested_at_ms: u64,
    phone: Option<&str>,
    result: Result<AuthorizedUser, ErrClass<P>>,
    now_ms: u64,
) -> Decision<C, P> {
    match result {
        Ok(user) => Decision {
            next: LoginPhase::Authorized,
            events: vec![login_complete_event(&user, false)],
        },
        Err(ErrClass::PasswordRequired(password_token)) => Decision {
            next: LoginPhase::PasswordPending {
                token: password_token,
                hint: None,
            },
            events: vec![awaiting_password_event(phone, None, false)],
        },
        Err(ErrClass::InvalidCode) => {
            let mut event = login_error_event(
                "sign_in",
                "invalid_or_expired_code",
                true,
                "invalid code".into(),
            );
            event.payload["code_age_seconds"] =
                json!(now_ms.saturating_sub(requested_at_ms) / 1000);
            event.payload["recovery"] = json!("request_new_code");
            Decision {
                next: LoginPhase::CodeSent {
                    token,
                    requested_at_ms,
                },
                events: vec![event],
            }
        }
        Err(err @ (ErrClass::Timeout | ErrClass::Transport(_))) => {
            let (reason, _, text) = describe(err);
            Decision {
                next: LoginPhase::CodeSent {
                    token,
                    requested_at_ms,
                },
                events: vec![login_error_event("sign_in", reason, true, text)],
            }
        }
        Err(err) => {
            let (_, _, text) = describe(err);
            let mut event = login_error_event(
                "sign_in",
                "sign_in_failed",
                false,
                format!("sign_in failed: {text}"),
            );
            event.payload["recovery"] = json!("restart_login");
            Decision {
                next: LoginPhase::Idle,
                events: vec![event],
            }
        }
    }
}

pub enum PasswordStep<C, P> {
    Decided(Decision<C, P>),
    /// The token was consumed by `check_password`; fetch a fresh one via
    /// `account.GetPassword`, then apply `decide_password_refetch`.
    RefetchToken {
        hint: Option<String>,
        reason: &'static str,
        error: String,
    },
}

pub fn decide_password_submit<C, P>(
    hint: Option<String>,
    result: Result<AuthorizedUser, ErrClass<P>>,
) -> PasswordStep<C, P> {
    match result {
        Ok(user) => PasswordStep::Decided(Decision {
            next: LoginPhase::Authorized,
            events: vec![login_complete_event(&user, false)],
        }),
        Err(ErrClass::InvalidPassword) => PasswordStep::RefetchToken {
            hint,
            reason: "invalid_password",
            error: "invalid password".into(),
        },
        Err(err @ (ErrClass::Timeout | ErrClass::Transport(_))) => {
            let (reason, _, text) = describe(err);
            PasswordStep::RefetchToken {
                hint,
                reason,
                error: text,
            }
        }
        Err(err) => {
            let (_, _, text) = describe(err);
            PasswordStep::Decided(Decision {
                next: LoginPhase::Idle,
                events: vec![login_error_event(
                    "check_password",
                    "check_password_failed",
                    false,
                    format!("check_password failed: {text}"),
                )],
            })
        }
    }
}

pub fn decide_password_refetch<C, P>(
    hint: Option<String>,
    reason: &'static str,
    error: String,
    result: Result<P, String>,
) -> Decision<C, P> {
    match result {
        Ok(token) => Decision {
            next: LoginPhase::PasswordPending { token, hint },
            events: vec![login_error_event("check_password", reason, true, error)],
        },
        Err(refetch_error) => Decision {
            next: LoginPhase::Idle,
            events: vec![login_error_event(
                "check_password",
                "password_retry_setup_failed",
                false,
                format!("{error}; refreshing the password challenge failed: {refetch_error}"),
            )],
        },
    }
}

/// (reason, retryable, display text) for an ErrClass — internal helper.
fn describe<P>(err: ErrClass<P>) -> (&'static str, bool, String) {
    match err {
        ErrClass::InvalidCode => ("invalid_or_expired_code", true, "invalid code".into()),
        ErrClass::InvalidPassword => ("invalid_password", true, "invalid password".into()),
        ErrClass::PasswordRequired(_) => {
            ("password_required", true, "2FA password required".into())
        }
        ErrClass::AuthRestart => (
            "auth_restart",
            false,
            "Telegram requested an auth restart".into(),
        ),
        ErrClass::Transport(text) => ("transport", true, text),
        ErrClass::Timeout => (
            "network_timeout",
            true,
            "Couldn't reach Telegram. Check your internet connection and try again.".into(),
        ),
        ErrClass::Fatal(text) => ("failed", false, text),
    }
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

    fn user() -> AuthorizedUser {
        AuthorizedUser {
            id: 42,
            first_name: Some("Ann".into()),
        }
    }

    // --- start ---

    #[test]
    fn start_ok_moves_to_code_sent_and_announces_code() {
        let StartStep::Decided(d) = decide_start::<u8, u8>("+1555", Ok(7), 0, 1_000) else {
            panic!("expected Decided")
        };
        assert!(matches!(
            d.next,
            LoginPhase::CodeSent {
                token: 7,
                requested_at_ms: 1_000
            }
        ));
        assert_eq!(d.events[0].kind, "login_awaiting_code");
        assert_eq!(d.events[0].payload["phone"], "+1555");
    }

    #[test]
    fn start_auth_restart_retries_once_with_fresh_session() {
        assert!(matches!(
            decide_start::<u8, u8>(
                "+1",
                Err((StartFailOp::RequestCode, ErrClass::AuthRestart)),
                0,
                0
            ),
            StartStep::RetryFreshSession
        ));
        // second attempt is terminal
        let StartStep::Decided(d) = decide_start::<u8, u8>(
            "+1",
            Err((StartFailOp::RequestCode, ErrClass::AuthRestart)),
            1,
            0,
        ) else {
            panic!()
        };
        assert!(matches!(d.next, LoginPhase::Idle));
        assert_eq!(d.events[0].payload["phase"], "request_code");
    }

    #[test]
    fn start_connect_timeout_is_retryable_and_stays_idle() {
        let StartStep::Decided(d) =
            decide_start::<u8, u8>("+1", Err((StartFailOp::Connect, ErrClass::Timeout)), 0, 0)
        else {
            panic!()
        };
        assert!(matches!(d.next, LoginPhase::Idle));
        assert_eq!(d.events[0].payload["phase"], "connect");
        assert_eq!(d.events[0].payload["reason"], "network_timeout");
        assert_eq!(d.events[0].payload["retryable"], true);
    }

    // --- submit_code ---

    #[test]
    fn code_ok_authorizes_and_emits_login_complete() {
        let d = decide_code_submit::<u8, u8>(7, 0, Some("+1"), Ok(user()), 1_000);
        assert!(d.next.is_authorized());
        assert_eq!(d.events[0].kind, "login_complete");
        assert_eq!(d.events[0].payload["user_id"], 42);
    }

    #[test]
    fn invalid_code_rearms_token_and_hints_new_code() {
        let d = decide_code_submit::<u8, u8>(7, 10_000, None, Err(ErrClass::InvalidCode), 190_000);
        // token survives for a retry with the same code hash
        assert!(matches!(
            d.next,
            LoginPhase::CodeSent {
                token: 7,
                requested_at_ms: 10_000
            }
        ));
        let payload = &d.events[0].payload;
        assert_eq!(payload["reason"], "invalid_or_expired_code");
        assert_eq!(payload["retryable"], true);
        assert_eq!(payload["code_age_seconds"], 180);
        assert_eq!(payload["recovery"], "request_new_code");
    }

    #[test]
    fn code_timeout_and_transport_keep_token() {
        for err in [ErrClass::<u8>::Timeout, ErrClass::Transport("reset".into())] {
            let d = decide_code_submit::<u8, u8>(7, 0, None, Err(err), 0);
            assert!(matches!(d.next, LoginPhase::CodeSent { token: 7, .. }));
            assert_eq!(d.events[0].payload["retryable"], true);
        }
    }

    #[test]
    fn code_password_required_moves_to_password_pending() {
        let d =
            decide_code_submit::<u8, u8>(7, 0, Some("+1"), Err(ErrClass::PasswordRequired(9)), 0);
        assert!(matches!(
            d.next,
            LoginPhase::PasswordPending {
                token: 9,
                hint: None
            }
        ));
        assert_eq!(d.events[0].kind, "login_awaiting_password");
        assert_eq!(d.events[0].payload["phone"], "+1");
    }

    #[test]
    fn code_fatal_clears_to_idle_with_restart_recovery() {
        let d = decide_code_submit::<u8, u8>(7, 0, None, Err(ErrClass::Fatal("boom".into())), 0);
        assert!(matches!(d.next, LoginPhase::Idle));
        assert_eq!(d.events[0].payload["retryable"], false);
        assert_eq!(d.events[0].payload["recovery"], "restart_login");
    }

    // --- submit_password ---

    #[test]
    fn password_ok_authorizes() {
        let PasswordStep::Decided(d) = decide_password_submit::<u8, u8>(None, Ok(user())) else {
            panic!()
        };
        assert!(d.next.is_authorized());
        assert_eq!(d.events[0].kind, "login_complete");
    }

    #[test]
    fn invalid_password_requests_token_refetch() {
        let PasswordStep::RefetchToken { hint, reason, .. } =
            decide_password_submit::<u8, u8>(Some("h".into()), Err(ErrClass::InvalidPassword))
        else {
            panic!("expected RefetchToken")
        };
        assert_eq!(hint.as_deref(), Some("h"));
        assert_eq!(reason, "invalid_password");
    }

    #[test]
    fn password_network_failures_also_need_refetch() {
        // check_password consumed the token even on timeout — retry needs a fresh one
        for err in [ErrClass::<u8>::Timeout, ErrClass::Transport("eof".into())] {
            assert!(matches!(
                decide_password_submit::<u8, u8>(None, Err(err)),
                PasswordStep::RefetchToken { .. }
            ));
        }
    }

    #[test]
    fn password_refetch_ok_rearms_and_stays_retryable() {
        let d = decide_password_refetch::<u8, u8>(
            Some("h".into()),
            "invalid_password",
            "bad pw".into(),
            Ok(9),
        );
        assert!(matches!(
            d.next,
            LoginPhase::PasswordPending { token: 9, .. }
        ));
        assert_eq!(d.events[0].payload["reason"], "invalid_password");
        assert_eq!(d.events[0].payload["retryable"], true);
    }

    #[test]
    fn password_refetch_failure_is_terminal() {
        let d = decide_password_refetch::<u8, u8>(
            None,
            "invalid_password",
            "bad pw".into(),
            Err("net down".into()),
        );
        assert!(matches!(d.next, LoginPhase::Idle));
        assert_eq!(d.events[0].payload["retryable"], false);
    }
}
