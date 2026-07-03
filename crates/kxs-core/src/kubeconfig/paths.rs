use std::ffi::OsStr;
use std::path::PathBuf;

pub fn default_kubeconfig_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".kube").join("config"))
}

/// Pure resolution logic, testable without touching process env.
pub fn kubeconfig_paths_from(
    env_val: Option<&OsStr>,
    home_default: Option<PathBuf>,
) -> Vec<PathBuf> {
    if let Some(v) = env_val {
        let paths: Vec<PathBuf> = std::env::split_paths(v)
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        if !paths.is_empty() {
            return paths;
        }
    }
    home_default.into_iter().collect()
}

pub fn kubeconfig_paths() -> Vec<PathBuf> {
    kubeconfig_paths_from(
        std::env::var_os("KUBECONFIG").as_deref(),
        default_kubeconfig_path(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kubeconfig_env_overrides_home() {
        let paths = kubeconfig_paths_from(
            Some(std::ffi::OsStr::new("/tmp/a:/tmp/b")),
            Some(PathBuf::from("/home/x/.kube/config")),
        );
        assert_eq!(
            paths,
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
    }

    #[test]
    fn falls_back_to_home_default() {
        let home = PathBuf::from("/home/x/.kube/config");
        assert_eq!(kubeconfig_paths_from(None, Some(home.clone())), vec![home]);
    }

    #[test]
    fn empty_env_falls_back() {
        let home = PathBuf::from("/home/x/.kube/config");
        assert_eq!(
            kubeconfig_paths_from(Some(std::ffi::OsStr::new("")), Some(home.clone())),
            vec![home]
        );
    }
}
