use anyhow::{anyhow, Context, Result};
use puffer_config::{ensure_workspace_dirs, load_config, save_user_config, ConfigPaths};
use puffer_secrets::{SecretUpsert, SecretVault};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};
use uuid::Uuid;

const AGENTENV_LOGIN_URL: &str = "https://agentenv.io/login";
const AGENTENV_API_URL: &str = "https://agentenv.io";

pub(crate) fn login_with_agentenv() -> Result<()> {
    let workspace_root = std::env::current_dir()?;
    let paths = ConfigPaths::discover(&workspace_root);
    ensure_workspace_dirs(&paths)?;

    let callback_listener = CallbackListener::bind_localhost("/agentenv-callback")?;
    let state = Uuid::new_v4().to_string();
    let mut login_url = url::Url::parse(AGENTENV_LOGIN_URL)?;
    login_url
        .query_pairs_mut()
        .append_pair("cli_callback", callback_listener.redirect_uri())
        .append_pair("state", &state);

    if !open_browser(login_url.as_str()) {
        return Err(anyhow!(
            "could not open the system browser for AgentEnv login"
        ));
    }

    let callback = callback_listener
        .wait_for_callback_url(Duration::from_secs(180))?
        .ok_or_else(|| anyhow!("timed out waiting for AgentEnv login callback"))?;
    let (token, parsed_state) = parse_agentenv_callback(&callback)?;
    if parsed_state.as_deref() != Some(state.as_str()) {
        return Err(anyhow!("AgentEnv login state mismatch"));
    }
    validate_agentenv_access_token(&token)?;

    store_agentenv_access_token(
        &paths,
        token,
        "AgentEnv access token",
        "AgentEnv OAuth access token",
        "oauth",
    )?;

    Ok(())
}

fn store_agentenv_access_token(
    paths: &ConfigPaths,
    token: String,
    label: &str,
    description: &str,
    source: &str,
) -> Result<()> {
    let vault = SecretVault::open(SecretVault::default_path(&paths.user_config_dir))?;
    let secret = vault.put(SecretUpsert {
        id: None,
        label: label.to_string(),
        description: Some(description.to_string()),
        value: token,
        username: None,
        origin: Some(AGENTENV_API_URL.to_string()),
        source: source.to_string(),
    })?;

    let mut config = load_config(paths)?;
    let agentenv = config.remote.agentenv.get_or_insert_with(Default::default);
    agentenv.enabled = true;
    agentenv.api_url = AGENTENV_API_URL.to_string();
    agentenv.credential_secret_id = Some(secret.id);
    agentenv.auth_method = "access_token".to_string();
    save_user_config(paths, &config)?;
    Ok(())
}

fn validate_agentenv_access_token(token: &str) -> Result<()> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?
        .get(format!("{AGENTENV_API_URL}/v1/auth/profile"))
        .bearer_auth(token)
        .send()
        .context("validate AgentEnv access token")?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let text = response.text().unwrap_or_default();
    Err(anyhow!(
        "AgentEnv login returned a token, but profile validation failed with HTTP {status}: {text}"
    ))
}

struct CallbackListener {
    listener: TcpListener,
    host: String,
    port: u16,
    expected_path: String,
    redirect_uri: String,
}

impl CallbackListener {
    fn bind_localhost(path: &str) -> Result<Self> {
        let listener = TcpListener::bind(("localhost", 0))
            .with_context(|| format!("failed to bind callback listener for {path}"))?;
        listener.set_nonblocking(true)?;
        let port = listener
            .local_addr()
            .context("failed to read callback listener address")?
            .port();
        Ok(Self {
            listener,
            host: "localhost".to_string(),
            port,
            expected_path: path.to_string(),
            redirect_uri: format!("http://localhost:{port}{path}"),
        })
    }

    fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    fn wait_for_callback_url(&self, timeout: Duration) -> Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0_u8; 4096];
                    let bytes_read = stream.read(&mut buffer)?;
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    if let Some(callback_url) =
                        parse_callback_request(&request, &self.host, self.port, &self.expected_path)
                    {
                        let _ = stream.write_all(success_response().as_bytes());
                        return Ok(Some(callback_url));
                    }
                    let _ = stream.write_all(error_response().as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }
}

fn open_browser(url: &str) -> bool {
    tauri_plugin_opener::open_url(url, None::<&str>).is_ok()
}

fn parse_callback_request(
    request: &str,
    host: &str,
    port: u16,
    expected_path: &str,
) -> Option<String> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    if method != "GET" {
        return None;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != expected_path {
        return None;
    }
    let suffix = if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    };
    Some(format!("http://{host}:{port}{suffix}"))
}

fn parse_agentenv_callback(callback: &str) -> Result<(String, Option<String>)> {
    let url = url::Url::parse(callback).context("parse AgentEnv callback URL")?;
    let token = url
        .query_pairs()
        .find_map(|(key, value)| (key == "token").then(|| value.into_owned()))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("AgentEnv callback did not include an access token"))?;
    let state = url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()));
    Ok((token, state))
}

fn success_response() -> &'static str {
    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 74\r\n\r\n<html><body>Authentication completed. You can return to Puffer.</body></html>"
}

fn error_response() -> &'static str {
    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 53\r\n\r\n<html><body>Invalid callback for Puffer.</body></html>"
}
