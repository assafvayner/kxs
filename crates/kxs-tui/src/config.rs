//! Per-context TUI config: `$XDG_CONFIG_HOME/kxs/tui.toml`, fallback
//! `~/.config/kxs/tui.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextConfig {
    pub namespace: Option<String>,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub last_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub editor: String,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub metrics_interval_secs: u64,
    #[serde(default)]
    pub contexts: std::collections::BTreeMap<String, ContextConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: None,
            editor: String::new(),
            readonly: false,
            metrics_interval_secs: 15,
            contexts: Default::default(),
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("kxs").join("tui.toml"));
        }
    }
    dirs::config_dir().map(|d| d.join("kxs").join("tui.toml"))
}

/// Tolerant read: a bad file is a warning and defaults, never an error.
pub fn load() -> (Config, Option<String>) {
    let Some(path) = config_path() else {
        return (Config::default(), None);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str(&text) {
            Ok(cfg) => (cfg, None),
            Err(e) => (
                Config::default(),
                Some(format!("bad config {}: {e}", path.display())),
            ),
        },
        Err(_) => (Config::default(), None),
    }
}

pub fn write(cfg: &Config) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err("no config directory".into());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    // 0600 like kubeconfig itself; the file holds no secrets but context names
    std::fs::File::create(&path)
        .and_then(|mut f| {
            use std::io::Write;
            f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
            write!(f, "{text}")
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut cfg = Config {
            theme: Some("tokyo-night".into()),
            ..Default::default()
        };
        cfg.contexts.insert(
            "prod".into(),
            ContextConfig {
                namespace: Some("app".into()),
                favorites: vec!["default".into(), "kube-system".into()],
                last_kind: Some("deployments".into()),
            },
        );
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.theme.as_deref(), Some("tokyo-night"));
        assert_eq!(back.contexts["prod"].favorites.len(), 2);
        assert_eq!(
            back.contexts["prod"].last_kind.as_deref(),
            Some("deployments")
        );
    }

    #[test]
    fn bad_file_falls_back_to_defaults() {
        let cfg: Result<Config, _> = toml::from_str("theme = ");
        assert!(cfg.is_err());
    }

    #[test]
    fn unknown_fields_are_tolerated() {
        let cfg: Result<Config, _> = toml::from_str("future_field = 1\nmetrics_interval_secs = 5");
        assert!(cfg.is_ok());
        assert_eq!(cfg.unwrap().metrics_interval_secs, 5);
    }
}
