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

/// Where `ctrl-s` writes resource dumps, k9s' screendump directory.
pub fn dump_dir() -> Result<PathBuf, String> {
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("kxs").join("dumps"));
        }
    }
    dirs::data_local_dir()
        .map(|d| d.join("kxs").join("dumps"))
        .ok_or_else(|| "no local data directory for dumps".to_string())
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
    let path = config_path().ok_or("no config directory")?;
    write_to(cfg, &path)
}

/// Atomic write: temp file next to the target, 0600, then rename.
pub fn write_to(cfg: &Config, path: &std::path::Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("toml.tmp");
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
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

    #[test]
    fn write_is_atomic_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kxs").join("tui.toml");
        write_to(&Config::default(), &path).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        assert!(!dir.path().join("kxs").join("tui.toml.tmp").exists());
    }
}
