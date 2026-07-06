//! Telegram QR-login support.
//!
//! Grammers 0.7 does not expose a high-level QR login helper, but the pinned
//! TL schema includes the raw `auth.exportLoginToken` and
//! `auth.importLoginToken` calls. This module wraps those calls behind the
//! subscriber command protocol and persists the same session file used by the
//! phone-code login path. The attempt's client and phase live on the shared
//! [`LoginSession`]; every MTProto round-trip is bounded at
//! [`LOGIN_NETWORK_TIMEOUT`].

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use grammers_client::{session::Session, types::User, Client, Config, InvocationError};
use grammers_tl_types as tl;
use serde_json::json;
use tokio::time::{timeout, timeout_at, Instant};
use tracing::{info, warn};

use crate::events::emit_control;
use crate::login::{self, LiveErr, LoginSession, LOGIN_NETWORK_TIMEOUT};
use crate::login_flow::{self, ErrClass, LoginPhase, TELEGRAM_UNREACHABLE_MESSAGE};
use crate::state::{default_init_params, resolve_api_credentials, PersistedCredentials, SkillEnv};

const DEFAULT_QR_WAIT_SECONDS: u64 = 120;
const DEFAULT_DC_ID: i32 = 2;

const MAX_QR_MIGRATIONS: usize = 4;
const MAX_QR_IMPORT_TOKEN_REFRESHES: usize = 2;

fn emit_control_spec(
    session: &LoginSession,
    spec: login_flow::ControlEventSpec,
) -> anyhow::Result<()> {
    emit_control(&session.env.topic, spec.kind, spec.payload)
}

/// Terminal QR failure: tear down the in-flight attempt entirely (client +
/// phase) so no later command can act on a half-dead connection, then emit
/// the error event.
fn fail(session: &mut LoginSession, event: login_flow::ControlEventSpec) -> anyhow::Result<()> {
    session.client = None;
    session.phase = LoginPhase::Idle;
    emit_control_spec(session, event)
}

/// Renders a classified bounded-call failure for QR error payloads.
fn qr_err_text(err: LiveErr) -> String {
    match err {
        ErrClass::Timeout => TELEGRAM_UNREACHABLE_MESSAGE.to_string(),
        ErrClass::Transport(text) | ErrClass::Fatal(text) => text,
        ErrClass::AuthRestart => "Telegram requested an auth restart".to_string(),
        _ => "QR login failed".to_string(),
    }
}

/// Starts QR login and emits either `login_qr`, `login_complete`, or
/// `login_error`.
pub async fn start(
    session: &mut LoginSession,
    api_id: Option<i32>,
    api_hash: Option<String>,
) -> anyhow::Result<()> {
    session.phase = LoginPhase::Idle;
    session.client = None;
    let persisted = PersistedCredentials::load(&session.env.credentials_path()).unwrap_or_default();
    let (api_id, api_hash) = match resolve_api_credentials(api_id, api_hash, &persisted) {
        Ok(pair) => pair,
        Err(error) => {
            warn!(%error, "telegram qr credential resolution failed");
            return emit_control_spec(
                session,
                login_flow::login_error_event(
                    "qr_credentials",
                    "qr_failed",
                    false,
                    error.to_string(),
                ),
            );
        }
    };
    session.api_id = Some(api_id);
    session.api_hash = Some(api_hash.clone());

    // `connect_qr_client` / `export_login_token` bound their own MTProto
    // round-trips (see `LOGIN_NETWORK_TIMEOUT`), so an unreachable Telegram
    // surfaces here as a normal `Err` carrying the friendly message rather
    // than hanging.
    let client = match connect_qr_client(api_id, api_hash.clone(), None).await {
        Ok(client) => client,
        Err(error) => {
            warn!(%error, "telegram qr connect failed");
            return emit_control_spec(
                session,
                login_flow::login_error_event(
                    "qr_connect",
                    "qr_failed",
                    false,
                    format!("connect failed: {error:#}"),
                ),
            );
        }
    };

    let token = match export_login_token(&client, api_id, &api_hash).await {
        Ok(token) => token,
        Err(error) => {
            warn!(%error, "telegram qr export token failed");
            return emit_control_spec(
                session,
                login_flow::login_error_event(
                    "qr_export",
                    "qr_failed",
                    false,
                    format!("export login token failed: {error:#}"),
                ),
            );
        }
    };

    // While the phase is `QrPending`, this handle is the (unauthenticated)
    // QR-attempt client. A runtime re-login therefore repoints the session's
    // client away from the previously authorized handle until the QR flow
    // completes or fails; business commands routed through the session are
    // rejected by Telegram during that window. Accepted: re-login normally
    // happens because the old authorization is already broken.
    session.client = Some(client);
    handle_login_token(session, DEFAULT_DC_ID, 0, token).await
}

/// Waits for approval of the active QR login. If the token expires before
/// approval, this emits a refreshed `login_qr` and keeps the QR phase alive.
pub async fn wait(session: &mut LoginSession, timeout_seconds: Option<u64>) -> anyhow::Result<()> {
    let (dc_id, refreshes) = match &session.phase {
        LoginPhase::QrPending {
            dc_id, refreshes, ..
        } => (*dc_id, usize::from(*refreshes)),
        other => {
            let name = other.name();
            return emit_control_spec(
                session,
                login_flow::wrong_phase_error("login_qr_wait", name),
            );
        }
    };
    let Some(client) = session.client.clone() else {
        return emit_control_spec(
            session,
            login_flow::wrong_phase_error("login_qr_wait", "no_client"),
        );
    };
    let (Some(api_id), Some(api_hash)) = (session.api_id, session.api_hash.clone()) else {
        return emit_control_spec(
            session,
            login_flow::wrong_phase_error("login_qr_wait", "no_credentials"),
        );
    };
    // Mirror the old `state.take()`: the pending QR attempt is consumed here;
    // the Token arm of `handle_login_token` re-arms `QrPending` whenever a
    // fresh QR goes out.
    session.phase = LoginPhase::Idle;

    let seconds = timeout_seconds.unwrap_or(DEFAULT_QR_WAIT_SECONDS).max(1);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        match timeout_at(deadline, client.next_raw_update()).await {
            Ok(Ok((tl::enums::Update::LoginToken, _))) => {
                let token = match export_login_token(&client, api_id, &api_hash).await {
                    Ok(token) => token,
                    Err(error) => {
                        warn!(%error, "telegram qr export after update failed");
                        return fail(
                            session,
                            login_flow::login_error_event(
                                "qr_export_after_update",
                                "qr_failed",
                                false,
                                format!(
                                    "export login token failed after approval update: {error:#}"
                                ),
                            ),
                        );
                    }
                };
                return handle_login_token(session, dc_id, refreshes, token).await;
            }
            Ok(Ok((_update, _))) => continue,
            Ok(Err(error)) => {
                warn!(%error, "telegram qr wait failed");
                return fail(
                    session,
                    login_flow::login_error_event(
                        "qr_wait",
                        "qr_failed",
                        false,
                        format!("QR login wait failed: {error:#}"),
                    ),
                );
            }
            Err(_) => {
                let token = match export_login_token(&client, api_id, &api_hash).await {
                    Ok(token) => token,
                    Err(error) => {
                        warn!(%error, "telegram qr refresh after timeout failed");
                        return fail(
                            session,
                            login_flow::login_error_event(
                                "qr_timeout",
                                "qr_failed",
                                false,
                                format!("QR login timed out and refresh failed: {error:#}"),
                            ),
                        );
                    }
                };
                return handle_login_token(session, dc_id, refreshes, token).await;
            }
        }
    }
}

async fn handle_login_token(
    session: &mut LoginSession,
    mut dc_id: i32,
    mut refreshes: usize,
    mut token: tl::enums::auth::LoginToken,
) -> anyhow::Result<()> {
    let api_id = session.api_id.context("qr login missing api_id")?;
    let api_hash = session
        .api_hash
        .clone()
        .context("qr login missing api_hash")?;
    for _ in 0..MAX_QR_MIGRATIONS {
        match token {
            tl::enums::auth::LoginToken::Token(login_token) => {
                emit_qr_token(&session.env, &login_token, None)?;
                session.phase = LoginPhase::QrPending {
                    dc_id,
                    refreshes: refreshes as u8,
                };
                return Ok(());
            }
            tl::enums::auth::LoginToken::Success(success) => {
                return complete_qr_login(session, dc_id, success.authorization).await;
            }
            tl::enums::auth::LoginToken::MigrateTo(migration) => {
                let client = match connect_qr_client(
                    api_id,
                    api_hash.clone(),
                    Some(migration.dc_id),
                )
                .await
                {
                    Ok(client) => client,
                    Err(error) => {
                        warn!(%error, dc_id = migration.dc_id, "telegram qr dc migration connect failed");
                        return fail(
                            session,
                            login_flow::login_error_event(
                                "qr_migrate",
                                "qr_failed",
                                false,
                                format!(
                                    "connect to Telegram DC {} failed: {error:#}",
                                    migration.dc_id
                                ),
                            ),
                        );
                    }
                };
                let import_result = match timeout(
                    LOGIN_NETWORK_TIMEOUT,
                    client.invoke(&tl::functions::auth::ImportLoginToken {
                        token: migration.token,
                    }),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        warn!(
                            timeout_secs = LOGIN_NETWORK_TIMEOUT.as_secs(),
                            dc_id = migration.dc_id,
                            "telegram qr import login token timed out"
                        );
                        return fail(
                            session,
                            login_flow::login_error_event(
                                "qr_migrate_timeout",
                                "qr_failed",
                                false,
                                TELEGRAM_UNREACHABLE_MESSAGE.to_string(),
                            ),
                        );
                    }
                };
                token = match import_result {
                    Ok(token) => token,
                    Err(error) => match classify_qr_import_invocation_error(&error, refreshes) {
                        QrImportErrorAction::AwaitPassword => {
                            return prepare_qr_password_challenge(session, client).await;
                        }
                        QrImportErrorAction::RefreshToken => {
                            refreshes += 1;
                            warn!(
                                %error,
                                dc_id = migration.dc_id,
                                refreshes,
                                "telegram qr import login token expired; issuing a new QR token"
                            );
                            let refresh_client =
                                session.client.clone().context("qr login missing client")?;
                            match export_login_token(&refresh_client, api_id, &api_hash).await {
                                Ok(tl::enums::auth::LoginToken::Token(login_token)) => {
                                    emit_qr_token(
                                        &session.env,
                                        &login_token,
                                        Some("auth_token_expired"),
                                    )?;
                                    session.phase = LoginPhase::QrPending {
                                        dc_id,
                                        refreshes: refreshes as u8,
                                    };
                                    return Ok(());
                                }
                                Ok(next_token) => {
                                    token = next_token;
                                    continue;
                                }
                                Err(refresh_error) => {
                                    warn!(
                                        error = %refresh_error,
                                        dc_id = migration.dc_id,
                                        "telegram qr import-expiry refresh failed"
                                    );
                                    return fail(
                                            session,
                                            login_flow::login_error_event(
                                                "qr_import_refresh",
                                                "qr_failed",
                                                false,
                                                format!(
                                                    "import login token in Telegram DC {} expired, and refreshing the QR token failed: {refresh_error:#}",
                                                    migration.dc_id
                                                ),
                                            ),
                                        );
                                }
                            }
                        }
                        QrImportErrorAction::Fail => {
                            warn!(%error, dc_id = migration.dc_id, "telegram qr import login token failed");
                            return fail(
                                session,
                                login_flow::login_error_event(
                                    "qr_import",
                                    "qr_failed",
                                    false,
                                    format!(
                                        "import login token in Telegram DC {} failed: {error:#}",
                                        migration.dc_id
                                    ),
                                ),
                            );
                        }
                    },
                };
                session.client = Some(client);
                dc_id = migration.dc_id;
            }
        }
    }

    fail(
        session,
        login_flow::login_error_event(
            "qr_migrate",
            "qr_failed",
            false,
            "Telegram QR login bounced through too many datacenters".to_string(),
        ),
    )
}

#[derive(Debug, PartialEq, Eq)]
enum QrImportErrorAction {
    AwaitPassword,
    RefreshToken,
    Fail,
}

fn classify_qr_import_invocation_error(
    error: &InvocationError,
    import_token_expired_refreshes: usize,
) -> QrImportErrorAction {
    if error.is("SESSION_PASSWORD_NEEDED") {
        return QrImportErrorAction::AwaitPassword;
    }
    if error.is("AUTH_TOKEN_EXPIRED") {
        return classify_qr_import_error("AUTH_TOKEN_EXPIRED", import_token_expired_refreshes);
    }
    QrImportErrorAction::Fail
}

fn classify_qr_import_error(
    error_name: &str,
    import_token_expired_refreshes: usize,
) -> QrImportErrorAction {
    if error_name == "AUTH_TOKEN_EXPIRED"
        && import_token_expired_refreshes < MAX_QR_IMPORT_TOKEN_REFRESHES
    {
        QrImportErrorAction::RefreshToken
    } else {
        QrImportErrorAction::Fail
    }
}

async fn complete_qr_login(
    session: &mut LoginSession,
    dc_id: i32,
    authorization: tl::enums::auth::Authorization,
) -> anyhow::Result<()> {
    let user = match authorization {
        tl::enums::auth::Authorization::Authorization(auth) => User::from_raw(auth.user),
        tl::enums::auth::Authorization::SignUpRequired(_) => {
            return fail(
                session,
                login_flow::login_error_event(
                    "qr_complete",
                    "qr_failed",
                    false,
                    "Telegram QR login returned sign-up required; use an official Telegram app to create the account first"
                        .to_string(),
                ),
            );
        }
    };
    let client = session.client.clone().context("qr login missing client")?;
    let api_id = session.api_id.context("qr login missing api_id")?;
    let api_hash = session
        .api_hash
        .clone()
        .context("qr login missing api_hash")?;

    client.session().set_user(user.id(), dc_id, user.is_bot());
    login::save_session(&session.env, &client)?;
    persist_qr_credentials(&session.env, api_id, api_hash.clone());

    let verified = reconnect_authorized_client(&session.env, api_id, api_hash).await?;
    let verified_user = login::bounded(verified.get_me()).await.map_err(|error| {
        anyhow::anyhow!("fetch QR-verified Telegram profile: {}", qr_err_text(error))
    })?;
    // Promote the staged session BEFORE `login_complete` goes out: parents
    // treat that event as terminal and may kill this process immediately
    // after reading it (agentenv/monorepo#551).
    login::promote_completed_session(&session.env)?;
    session.client = Some(verified);
    session.phase = LoginPhase::Authorized;
    let user_data = login_flow::AuthorizedUser {
        id: verified_user.id(),
        first_name: Some(verified_user.first_name().to_string()),
    };
    emit_control_spec(session, login_flow::login_complete_event(&user_data, true))?;
    info!(user_id = verified_user.id(), "telegram qr login complete");
    Ok(())
}

async fn prepare_qr_password_challenge(
    session: &mut LoginSession,
    client: Client,
) -> anyhow::Result<()> {
    let password_token = match login::get_password_token(&client).await {
        Ok(token) => token,
        Err(error) => {
            let text = qr_err_text(error);
            warn!(error = %text, "telegram qr password token fetch failed");
            return fail(
                session,
                login_flow::login_error_event(
                    "qr_password",
                    "qr_failed",
                    false,
                    format!(
                        "Telegram QR login requires 2FA, but password challenge setup failed: {text}"
                    ),
                ),
            );
        }
    };
    let hint = password_token.hint().map(str::to_string);
    if let Err(error) = login::save_session(&session.env, &client) {
        warn!(error = %error, "failed to persist telegram qr 2FA session");
    }
    session.client = Some(client);
    session.phase = LoginPhase::PasswordPending {
        token: password_token,
        hint: hint.clone(),
    };
    emit_control_spec(
        session,
        login_flow::awaiting_password_event(None, hint.as_deref(), true),
    )?;
    info!("telegram qr login requires 2FA password");
    Ok(())
}

async fn connect_qr_client(
    api_id: i32,
    api_hash: String,
    force_dc_id: Option<i32>,
) -> anyhow::Result<Client> {
    let session = Session::new();
    if let Some(dc_id) = force_dc_id {
        session.set_user(0, dc_id, false);
    }
    login::bounded(Client::connect(Config {
        session,
        api_id,
        api_hash,
        params: default_init_params(),
    }))
    .await
    .map_err(|error| anyhow::anyhow!("{}", qr_err_text(error)))
}

async fn reconnect_authorized_client(
    env: &SkillEnv,
    api_id: i32,
    api_hash: String,
) -> anyhow::Result<Client> {
    let session = Session::load_file_or_create(&env.session_path)
        .with_context(|| format!("load session file {}", env.session_path.display()))?;
    let client = login::bounded(Client::connect(Config {
        session,
        api_id,
        api_hash,
        params: default_init_params(),
    }))
    .await
    .map_err(|error| {
        anyhow::anyhow!(
            "reconnect authorized Telegram QR session: {}",
            qr_err_text(error)
        )
    })?;
    let authorized = login::bounded(client.is_authorized())
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "verify Telegram QR session authorization: {}",
                qr_err_text(error)
            )
        })?;
    if !authorized {
        anyhow::bail!("Telegram did not accept the QR-authorized session");
    }
    Ok(client)
}

async fn export_login_token(
    client: &Client,
    api_id: i32,
    api_hash: &str,
) -> anyhow::Result<tl::enums::auth::LoginToken> {
    login::bounded(client.invoke(&tl::functions::auth::ExportLoginToken {
        api_id,
        api_hash: api_hash.to_string(),
        except_ids: Vec::new(),
    }))
    .await
    .map_err(|error| anyhow::anyhow!("{}", qr_err_text(error)))
}

fn emit_qr_token(
    env: &SkillEnv,
    login_token: &tl::types::auth::LoginToken,
    refresh_reason: Option<&'static str>,
) -> anyhow::Result<()> {
    let url = qr_login_url(&login_token.token);
    let mut payload = json!({
        "url": url,
        "expires_at_unix": login_token.expires,
        "expires_in_seconds": seconds_until(login_token.expires),
        "next": "Open this URL from a logged-in Telegram app, approve the login, then run `telegram login-qr-wait`."
    });
    if let Some(reason) = refresh_reason {
        payload["refresh_reason"] = json!(reason);
        if reason == "auth_token_expired" {
            payload["message"] = json!("The QR code expired after approval. Scan the new QR code.");
        }
    }
    emit_control(&env.topic, "login_qr", payload)
}

fn qr_login_url(token: &[u8]) -> String {
    format!("tg://login?token={}", URL_SAFE_NO_PAD.encode(token))
}

fn seconds_until(expires_at_unix: i32) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    i64::from(expires_at_unix)
        .checked_sub(now)
        .unwrap_or(0)
        .max(0)
}

fn persist_qr_credentials(env: &SkillEnv, api_id: i32, api_hash: String) {
    let creds = PersistedCredentials {
        api_id: Some(api_id),
        api_hash: Some(api_hash),
        phone: None,
    };
    if let Err(error) = creds.save(&env.credentials_path()) {
        warn!(error = %error, "failed to persist telegram qr credentials");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_login_url_uses_url_safe_unpadded_base64() {
        assert_eq!(qr_login_url(&[251, 255, 16]), "tg://login?token=-_8Q");
    }

    #[test]
    fn seconds_until_saturates_for_past_expiration() {
        assert_eq!(seconds_until(1), 0);
    }

    #[test]
    fn auth_token_expired_import_error_refreshes_until_limit() {
        assert_eq!(
            classify_qr_import_error("AUTH_TOKEN_EXPIRED", 0),
            QrImportErrorAction::RefreshToken
        );
        assert_eq!(
            classify_qr_import_error("AUTH_TOKEN_EXPIRED", MAX_QR_IMPORT_TOKEN_REFRESHES),
            QrImportErrorAction::Fail
        );
    }

    #[test]
    fn non_expired_qr_import_errors_remain_terminal() {
        assert_eq!(
            classify_qr_import_error("AUTH_TOKEN_INVALID", 0),
            QrImportErrorAction::Fail
        );
        assert_eq!(
            classify_qr_import_error("NETWORK_MIGRATE_5", 0),
            QrImportErrorAction::Fail
        );
    }
}
