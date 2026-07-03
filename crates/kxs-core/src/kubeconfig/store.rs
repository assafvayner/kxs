use crate::error::Result;
use crate::kubeconfig::io::read_file;
use crate::kubeconfig::types::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct KubeconfigFile {
    pub path: PathBuf,
    pub config: Kubeconfig,
    /// false when the path didn't exist at read time
    pub exists: bool,
    /// Set when the on-disk file is currently malformed; `config` then holds
    /// the last successfully parsed state (or empty at initial load).
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct KubeconfigStore {
    files: Vec<KubeconfigFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextSummary {
    pub name: String,
    pub cluster: String,
    pub user: String,
    pub namespace: Option<String>,
    pub source: PathBuf,
}

impl KubeconfigStore {
    pub fn load(paths: Vec<PathBuf>) -> Result<Self> {
        let mut files = Vec::new();
        for path in paths {
            files.push(read_file(&path)?);
        }
        Ok(Self { files })
    }

    /// Like `load`, but malformed files become warnings instead of failing the app.
    pub fn load_tolerant(paths: Vec<PathBuf>) -> (Self, Vec<String>) {
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        for path in paths {
            match read_file(&path) {
                Ok(f) => files.push(f),
                Err(e) => {
                    let msg = e.to_string();
                    warnings.push(msg.clone());
                    files.push(KubeconfigFile {
                        path,
                        config: Kubeconfig::default(),
                        exists: true,
                        error: Some(msg),
                    });
                }
            }
        }
        (Self { files }, warnings)
    }

    /// Re-reads every file. On failure the previous good state for that file
    /// is kept and a warning is returned — suits the file-watcher reload path.
    pub fn reload(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        for f in &mut self.files {
            match read_file(&f.path) {
                Ok(fresh) => *f = fresh,
                Err(e) => {
                    let msg = e.to_string();
                    warnings.push(msg.clone());
                    f.error = Some(msg);
                }
            }
        }
        warnings
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }

    /// Merged view. kubectl precedence: across files the first file wins;
    /// within a file the last duplicate wins.
    pub fn contexts(&self) -> Vec<ContextSummary> {
        let mut claimed: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        for f in &self.files {
            let mut file_slots: HashMap<String, usize> = HashMap::new();
            for nc in &f.config.contexts {
                if claimed.contains(&nc.name) {
                    continue;
                }
                let summary = ContextSummary {
                    name: nc.name.clone(),
                    cluster: nc.context.cluster.clone(),
                    user: nc.context.user.clone(),
                    namespace: nc.context.namespace.clone(),
                    source: f.path.clone(),
                };
                match file_slots.get(&nc.name) {
                    Some(&i) => out[i] = summary,
                    None => {
                        file_slots.insert(nc.name.clone(), out.len());
                        out.push(summary);
                    }
                }
            }
            claimed.extend(file_slots.into_keys());
        }
        out
    }

    pub fn current_context(&self) -> Option<String> {
        self.files
            .iter()
            .find_map(|f| f.config.current_context.clone())
    }

    // within a file the last duplicate wins (kubectl behavior); across files the first file wins
    pub fn find_cluster(&self, name: &str) -> Option<(&Path, &NamedCluster)> {
        self.files.iter().find_map(|f| {
            f.config
                .clusters
                .iter()
                .rev()
                .find(|c| c.name == name)
                .map(|c| (f.path.as_path(), c))
        })
    }

    pub fn find_user(&self, name: &str) -> Option<(&Path, &NamedAuthInfo)> {
        self.files.iter().find_map(|f| {
            f.config
                .users
                .iter()
                .rev()
                .find(|u| u.name == name)
                .map(|u| (f.path.as_path(), u))
        })
    }

    pub fn find_context(&self, name: &str) -> Option<(&Path, &NamedContext)> {
        self.files.iter().find_map(|f| {
            f.config
                .contexts
                .iter()
                .rev()
                .find(|c| c.name == name)
                .map(|c| (f.path.as_path(), c))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn write(dir: &tempfile::TempDir, name: &str, content: &str) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    pub(crate) const FILE_A: &str = r#"
current-context: prod
clusters: [{name: prod, cluster: {server: "https://a", my-extra: keep}}]
users: [{name: u1, user: {token: t}}]
contexts: [{name: prod, context: {cluster: prod, user: u1, namespace: scanner}}]
"#;

    pub(crate) const FILE_B: &str = r#"
clusters: [{name: prod, cluster: {server: "https://b"}}, {name: dev, cluster: {server: "https://dev"}}]
users: [{name: u2, user: {token: t2}}]
contexts: [{name: prod, context: {cluster: prod, user: u2}}, {name: dev, context: {cluster: dev, user: u2}}]
"#;

    #[test]
    fn merges_files_first_wins() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.yaml", FILE_A);
        let b = write(&dir, "b.yaml", FILE_B);
        let store = KubeconfigStore::load(vec![a.clone(), b.clone()]).unwrap();
        let ctxs = store.contexts();
        assert_eq!(ctxs.len(), 2);
        assert_eq!(ctxs[0].name, "prod");
        assert_eq!(ctxs[0].source, a);
        assert_eq!(ctxs[0].namespace.as_deref(), Some("scanner"));
        assert_eq!(ctxs[1].name, "dev");
        assert_eq!(ctxs[1].source, b);
        assert_eq!(store.current_context().as_deref(), Some("prod"));
        // duplicate cluster name: first file wins
        let (src, cluster) = store.find_cluster("prod").unwrap();
        assert_eq!(src, a.as_path());
        assert_eq!(cluster.cluster.server.as_deref(), Some("https://a"));
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = KubeconfigStore::load(vec![dir.path().join("nope")]).unwrap();
        assert!(store.contexts().is_empty());
        assert!(store.current_context().is_none());
    }

    #[test]
    fn load_tolerant_skips_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let good = write(&dir, "good.yaml", FILE_A);
        let bad = write(&dir, "bad.yaml", "clusters: [not-a-mapping");
        let (store, warnings) = KubeconfigStore::load_tolerant(vec![good, bad]);
        assert_eq!(store.contexts().len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bad.yaml"));
    }

    #[test]
    fn reload_picks_up_changes() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.yaml", FILE_A);
        let mut store = KubeconfigStore::load(vec![a.clone()]).unwrap();
        write(&dir, "a.yaml", FILE_B);
        assert!(store.reload().is_empty());
        assert_eq!(store.contexts().len(), 2);
    }

    #[test]
    fn reload_keeps_previous_state_on_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.yaml", FILE_A);
        let mut store = KubeconfigStore::load(vec![a.clone()]).unwrap();
        write(&dir, "a.yaml", "clusters: [broken");
        let warnings = store.reload();
        assert_eq!(warnings.len(), 1);
        assert_eq!(store.contexts().len(), 1, "previous good state kept");
        write(&dir, "a.yaml", FILE_B);
        assert!(store.reload().is_empty());
        assert_eq!(store.contexts().len(), 2);
    }

    #[test]
    fn load_tolerant_keeps_malformed_path_watchable() {
        let dir = tempfile::tempdir().unwrap();
        let good = write(&dir, "good.yaml", FILE_A);
        let bad = write(&dir, "bad.yaml", "clusters: [not-a-mapping");
        let (mut store, _) = KubeconfigStore::load_tolerant(vec![good, bad.clone()]);
        assert_eq!(store.paths().len(), 2, "malformed path must stay tracked");
        write(&dir, "bad.yaml", FILE_B);
        assert!(store.reload().is_empty());
        assert_eq!(store.contexts().len(), 2);
    }

    #[test]
    fn within_file_duplicate_last_wins() {
        let dir = tempfile::tempdir().unwrap();
        let dup = write(
            &dir,
            "dup.yaml",
            r#"
clusters: [{name: c1, cluster: {server: "https://first"}}, {name: c1, cluster: {server: "https://second"}}]
users: [{name: u, user: {token: t}}]
contexts: [{name: x, context: {cluster: c1, user: u, namespace: one}}, {name: x, context: {cluster: c1, user: u, namespace: two}}]
"#,
        );
        let store = KubeconfigStore::load(vec![dup]).unwrap();
        assert_eq!(
            store
                .find_cluster("c1")
                .unwrap()
                .1
                .cluster
                .server
                .as_deref(),
            Some("https://second")
        );
        let ctxs = store.contexts();
        assert_eq!(ctxs.len(), 1);
        assert_eq!(ctxs[0].namespace.as_deref(), Some("two"));
    }
}
