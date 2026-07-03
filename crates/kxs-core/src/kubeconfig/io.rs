use crate::error::{Error, Result};
use crate::kubeconfig::store::KubeconfigFile;
use crate::kubeconfig::types::Kubeconfig;
use std::path::Path;

pub fn read_file(path: &Path) -> Result<KubeconfigFile> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let config: Kubeconfig = serde_yaml_ng::from_str(&s).map_err(|e| Error::Yaml {
                path: path.to_path_buf(),
                source: e,
            })?;
            Ok(KubeconfigFile {
                path: path.to_path_buf(),
                config,
                exists: true,
                error: None,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(KubeconfigFile {
            path: path.to_path_buf(),
            config: Kubeconfig::default(),
            exists: false,
            error: None,
        }),
        Err(e) => Err(Error::Io {
            path: path.to_path_buf(),
            source: e,
        }),
    }
}
