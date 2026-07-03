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

pub const BACKUP_KEEP: usize = 5;

/// Atomic write with timestamped backup of the previous content.
/// Backup names zero-pad nanos so lexicographic sort == chronological.
pub fn write_file(file: &KubeconfigFile) -> Result<()> {
    let path = &file.path;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io {
            path: path.clone(),
            source: e,
        })?;
    }
    if path.exists() {
        backup(path)?;
    }
    let yaml = serde_yaml_ng::to_string(&file.config).map_err(|e| Error::Yaml {
        path: path.clone(),
        source: e,
    })?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ".".into());
    let tmp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| Error::Io {
        path: path.clone(),
        source: e,
    })?;
    std::fs::write(tmp.path(), &yaml).map_err(|e| Error::Io {
        path: path.clone(),
        source: e,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode)).map_err(
            |e| Error::Io {
                path: path.clone(),
                source: e,
            },
        )?;
    }
    tmp.persist(path).map_err(|e| Error::Io {
        path: path.clone(),
        source: e.error,
    })?;
    Ok(())
}

fn backup(path: &Path) -> Result<()> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let file_name = path
        .file_name()
        .expect("kubeconfig path has file name")
        .to_string_lossy();
    let backup_path = path.with_file_name(format!("{file_name}.kxs-backup-{ts:030}"));
    std::fs::copy(path, &backup_path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    rotate(path)
}

fn rotate(path: &Path) -> Result<()> {
    let prefix = format!(
        "{}.kxs-backup-",
        path.file_name().expect("file name").to_string_lossy()
    );
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut backups: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Error::Io {
            path: dir.to_path_buf(),
            source: e,
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
        })
        .collect();
    backups.sort();
    while backups.len() > BACKUP_KEEP {
        let oldest = backups.remove(0);
        let _ = std::fs::remove_file(oldest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_creates_file_and_backups_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        for i in 0..7 {
            let mut file = read_file(&path).unwrap();
            file.config.current_context = Some(format!("v{i}"));
            write_file(&file).unwrap();
        }
        let cur = read_file(&path).unwrap();
        assert_eq!(cur.config.current_context.as_deref(), Some("v6"));

        let mut backups: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("config.kxs-backup-"))
            .collect();
        backups.sort();
        // writes 1..=6 back up v0..v5; rotation keeps the newest 5 => v1..v5
        assert_eq!(backups.len(), BACKUP_KEEP);
        let oldest = std::fs::read_to_string(dir.path().join(&backups[0])).unwrap();
        assert!(
            oldest.contains("v1"),
            "oldest surviving backup should be v1: {oldest}"
        );
        let newest = std::fs::read_to_string(dir.path().join(&backups[BACKUP_KEEP - 1])).unwrap();
        assert!(
            newest.contains("v5"),
            "newest backup should be v5: {newest}"
        );
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("dir").join("config");
        let file = read_file(&path).unwrap();
        write_file(&file).unwrap();
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, "apiVersion: v1\nkind: Config\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut f = read_file(&path).unwrap();
        f.config.current_context = Some("x".into());
        write_file(&f).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
