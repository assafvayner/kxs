use crate::error::Result;
use crate::kubeconfig::io::read_file;
use crate::kubeconfig::types::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct KubeconfigFile {
    pub path: PathBuf,
    pub config: Kubeconfig,
    /// false when the path didn't exist at read time
    pub exists: bool,
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
                Err(e) => warnings.push(e.to_string()),
            }
        }
        (Self { files }, warnings)
    }

    pub fn reload(&mut self) -> Result<()> {
        for f in &mut self.files {
            *f = read_file(&f.path)?;
        }
        Ok(())
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }

    /// Merged view. kubectl precedence: for duplicate names, the first file wins.
    pub fn contexts(&self) -> Vec<ContextSummary> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for f in &self.files {
            for nc in &f.config.contexts {
                if seen.insert(nc.name.clone()) {
                    out.push(ContextSummary {
                        name: nc.name.clone(),
                        cluster: nc.context.cluster.clone(),
                        user: nc.context.user.clone(),
                        namespace: nc.context.namespace.clone(),
                        source: f.path.clone(),
                    });
                }
            }
        }
        out
    }

    pub fn current_context(&self) -> Option<String> {
        self.files
            .iter()
            .find_map(|f| f.config.current_context.clone())
    }

    pub fn find_cluster(&self, name: &str) -> Option<(&Path, &NamedCluster)> {
        self.files.iter().find_map(|f| {
            f.config
                .clusters
                .iter()
                .find(|c| c.name == name)
                .map(|c| (f.path.as_path(), c))
        })
    }

    pub fn find_user(&self, name: &str) -> Option<(&Path, &NamedAuthInfo)> {
        self.files.iter().find_map(|f| {
            f.config
                .users
                .iter()
                .find(|u| u.name == name)
                .map(|u| (f.path.as_path(), u))
        })
    }

    pub fn find_context(&self, name: &str) -> Option<(&Path, &NamedContext)> {
        self.files.iter().find_map(|f| {
            f.config
                .contexts
                .iter()
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
        store.reload().unwrap();
        assert_eq!(store.contexts().len(), 2);
    }
}
