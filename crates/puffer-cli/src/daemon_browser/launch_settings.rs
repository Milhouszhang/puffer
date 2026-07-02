//! Browser extension launch settings derived from Puffer config.

use anyhow::Result;
use puffer_config::{
    builtin_captcha_solvers, stage_builtin_captcha_extension, CaptchaExtensionSeed, ConfigPaths,
    ProxyConfig, ProxyScheme, PufferConfig,
};
use puffer_secrets::SecretVault;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Effective browser extension state used when starting a browser root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BrowserLaunchSettings {
    extension_dirs: Vec<PathBuf>,
    seeds: Vec<CaptchaExtensionSeed>,
    proxy_args: Vec<String>,
}

impl BrowserLaunchSettings {
    /// Builds launch settings from the currently loaded daemon config.
    pub(crate) fn from_config(paths: &ConfigPaths, config: &PufferConfig) -> Result<Self> {
        let proxy_args = chrome_proxy_args(&config.network.proxy);
        let browser = &config.browser;
        if !browser.extensions_enabled {
            return Ok(Self {
                proxy_args,
                ..Self::default()
            });
        }

        let mut extension_dirs = Vec::new();
        for extension in browser
            .extensions
            .iter()
            .filter(|extension| extension.enabled)
        {
            push_extension_dir(&mut extension_dirs, PathBuf::from(&extension.path));
        }

        let mut seeds = Vec::new();
        if browser.captcha.enabled {
            if let Some(solver) = builtin_captcha_solvers()
                .iter()
                .find(|solver| solver.id == browser.captcha.selected_solver)
            {
                let configured = browser.captcha.solvers.get(solver.id);
                let source_dir = paths.builtin_resources_dir.join(solver.extension_path);
                let mut extension_dir = source_dir.clone();
                if let Some(secret_id) = configured.and_then(|item| item.api_key_secret_id.as_ref())
                {
                    if let Some(api_key) = reveal_secret_value(paths, secret_id) {
                        let base_url = configured
                            .and_then(|item| item.base_url.clone())
                            .unwrap_or_else(|| solver.default_base_url.to_string());
                        let seed = CaptchaExtensionSeed::new(solver.id, api_key, base_url);
                        extension_dir = stage_builtin_captcha_extension(
                            &source_dir,
                            &paths.user_config_dir.join("browser-extension-stage"),
                            &seed,
                        )?;
                        if seed.solver_id() != "nopecha" {
                            seeds.push(seed);
                        }
                    }
                }
                push_extension_dir(&mut extension_dirs, extension_dir);
            }
        }

        dedupe_extension_dirs(&mut extension_dirs);
        Ok(Self {
            extension_dirs,
            seeds,
            proxy_args,
        })
    }

    /// Returns unpacked extension directories that should be loaded by Chrome.
    pub(crate) fn extension_dirs(&self) -> &[PathBuf] {
        &self.extension_dirs
    }

    /// Returns extension local-storage seed values for bundled captcha solvers.
    pub(crate) fn seeds(&self) -> &[CaptchaExtensionSeed] {
        &self.seeds
    }

    /// Returns Chrome proxy flags to pass at launch. Empty when proxy is disabled.
    pub(crate) fn proxy_args(&self) -> &[String] {
        &self.proxy_args
    }

    /// Creates launch settings with extension directories for tests.
    #[cfg(test)]
    pub(crate) fn with_extension_dirs(extension_dirs: Vec<PathBuf>) -> Self {
        Self {
            extension_dirs,
            seeds: Vec::new(),
            proxy_args: Vec::new(),
        }
    }
}

fn push_extension_dir(extension_dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if extension_manifest_present(&path) {
        extension_dirs.push(path);
    }
}

fn extension_manifest_present(path: &Path) -> bool {
    path.join("manifest.json").is_file()
}

fn reveal_secret_value(paths: &ConfigPaths, secret_id: &str) -> Option<String> {
    let store_path = SecretVault::default_path(&paths.user_config_dir);
    let vault = SecretVault::open(store_path).ok()?;
    match vault.reveal(secret_id) {
        Ok(secret) => Some(secret.value),
        Err(error) => {
            eprintln!(
                "puffer browser: captcha API key `{secret_id}` could not be revealed: {error}"
            );
            None
        }
    }
}

fn dedupe_extension_dirs(extension_dirs: &mut Vec<PathBuf>) {
    let mut seen = BTreeSet::new();
    extension_dirs.retain(|path| seen.insert(path.clone()));
}

/// Chrome proxy flags derived from config. Empty when disabled. Authenticated
/// proxies are unsupported for browser sessions (Chrome --proxy-server cannot
/// carry inline credentials) — credentials are dropped from the server arg.
fn chrome_proxy_args(proxy: &ProxyConfig) -> Vec<String> {
    if !proxy.enabled {
        return Vec::new();
    }
    let Some(endpoint) = proxy.selected_endpoint() else {
        return Vec::new();
    };
    // Chrome's `--proxy-server` rejects the `socks5h` scheme; its `socks5://`
    // already performs remote DNS resolution, so map Socks5h down to socks5.
    // (reqwest/env `ALL_PROXY` keep socks5h — see `proxy_env_block`.)
    let scheme = match endpoint.scheme {
        ProxyScheme::Socks5h => "socks5",
        other => other.as_uri_scheme(),
    };
    let server = format!("{}://{}:{}", scheme, endpoint.host.trim(), endpoint.port);
    let mut bypass = vec!["<-loopback>".to_string()];
    bypass.extend(
        proxy
            .bypass
            .iter()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty()),
    );
    vec![
        format!("--proxy-server={server}"),
        format!("--proxy-bypass-list={}", bypass.join(";")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use puffer_config::{ProxyEndpoint, ProxyScheme};

    fn socks_proxy_config() -> ProxyConfig {
        ProxyConfig {
            enabled: true,
            selected: Some("p".into()),
            bypass: vec!["example.com".into()],
            proxies: vec![ProxyEndpoint {
                id: "p".into(),
                scheme: ProxyScheme::Socks5,
                host: "127.0.0.1".into(),
                port: 7890,
                username: None,
                password: None,
            }],
        }
    }

    #[test]
    fn chrome_proxy_args_enabled_emits_server_and_bypass() {
        let args = chrome_proxy_args(&socks_proxy_config());
        assert!(
            args.contains(&"--proxy-server=socks5://127.0.0.1:7890".to_string()),
            "expected --proxy-server arg, got: {args:?}"
        );
        assert!(
            args.iter().any(|a| a.starts_with("--proxy-bypass-list=")
                && a.contains("example.com")
                && a.contains("<-loopback>")),
            "expected --proxy-bypass-list with example.com and <-loopback>, got: {args:?}"
        );
    }

    #[test]
    fn chrome_proxy_args_maps_socks5h_to_socks5() {
        let mut cfg = socks_proxy_config();
        cfg.proxies[0].scheme = ProxyScheme::Socks5h;
        let args = chrome_proxy_args(&cfg);
        assert!(
            args.contains(&"--proxy-server=socks5://127.0.0.1:7890".to_string()),
            "Chrome rejects socks5h; expected socks5:// server arg, got: {args:?}"
        );
    }

    #[test]
    fn chrome_proxy_args_disabled_is_empty() {
        let cfg = ProxyConfig {
            enabled: false,
            ..socks_proxy_config()
        };
        assert!(
            chrome_proxy_args(&cfg).is_empty(),
            "expected empty args when proxy disabled"
        );
    }
}
