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

    /// Where new entries go when no target is given: ~/.kube/config if loaded,
    /// else the first loaded file.
    pub fn default_target(&self) -> PathBuf {
        if let Some(hd) = crate::kubeconfig::paths::default_kubeconfig_path() {
            if self.files.iter().any(|f| f.path == hd) {
                return hd;
            }
        }
        self.files
            .first()
            .map(|f| f.path.clone())
            .or_else(crate::kubeconfig::paths::default_kubeconfig_path)
            .expect("no kubeconfig path available")
    }

    /// Read the target file fresh from disk, apply `op`, write back, refresh memory.
    fn mutate<F>(&mut self, path: &Path, op: F) -> Result<()>
    where
        F: FnOnce(&mut Kubeconfig) -> Result<()>,
    {
        let mut file = crate::kubeconfig::io::read_file(path)?;
        op(&mut file.config)?;
        crate::kubeconfig::io::write_file(&file)?;
        let fresh = crate::kubeconfig::io::read_file(path)?;
        match self.files.iter_mut().find(|f| f.path == path) {
            Some(f) => *f = fresh,
            None => self.files.push(fresh),
        }
        Ok(())
    }

    pub fn upsert_cluster(
        &mut self,
        name: &str,
        mut cluster: Cluster,
        target: Option<&Path>,
    ) -> Result<()> {
        let found = self
            .find_cluster(name)
            .map(|(p, c)| (p.to_path_buf(), c.cluster.extras.clone()));
        if let Some((path, extras)) = found {
            cluster.extras = extras; // preserve unknown fields on edit
            let name = name.to_string();
            return self.mutate(&path, move |cfg| {
                let entry = cfg
                    .clusters
                    .iter_mut()
                    .rev()
                    .find(|c| c.name == name)
                    .ok_or_else(|| crate::error::Error::NotFound {
                        kind: "cluster",
                        name: name.clone(),
                    })?;
                entry.cluster = cluster;
                Ok(())
            });
        }
        let target = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.default_target());
        let name = name.to_string();
        self.mutate(&target, move |cfg| {
            cfg.clusters.push(NamedCluster {
                name,
                cluster,
                extras: Extras::new(),
            });
            Ok(())
        })
    }

    pub fn upsert_user(
        &mut self,
        name: &str,
        mut user: AuthInfo,
        target: Option<&Path>,
    ) -> Result<()> {
        let found = self
            .find_user(name)
            .map(|(p, u)| (p.to_path_buf(), u.user.extras.clone()));
        if let Some((path, extras)) = found {
            user.extras = extras;
            let name = name.to_string();
            return self.mutate(&path, move |cfg| {
                let entry = cfg
                    .users
                    .iter_mut()
                    .rev()
                    .find(|u| u.name == name)
                    .ok_or_else(|| crate::error::Error::NotFound {
                        kind: "user",
                        name: name.clone(),
                    })?;
                entry.user = user;
                Ok(())
            });
        }
        let target = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.default_target());
        let name = name.to_string();
        self.mutate(&target, move |cfg| {
            cfg.users.push(NamedAuthInfo {
                name,
                user,
                extras: Extras::new(),
            });
            Ok(())
        })
    }

    pub fn create_context(
        &mut self,
        name: &str,
        cluster: &str,
        user: &str,
        namespace: Option<String>,
        target: Option<&Path>,
    ) -> Result<()> {
        if self.find_context(name).is_some() {
            return Err(crate::error::Error::AlreadyExists {
                kind: "context",
                name: name.into(),
            });
        }
        if self.find_cluster(cluster).is_none() {
            return Err(crate::error::Error::NotFound {
                kind: "cluster",
                name: cluster.into(),
            });
        }
        if self.find_user(user).is_none() {
            return Err(crate::error::Error::NotFound {
                kind: "user",
                name: user.into(),
            });
        }
        let target = target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.default_target());
        let (name, cluster, user) = (name.to_string(), cluster.to_string(), user.to_string());
        self.mutate(&target, move |cfg| {
            cfg.contexts.push(NamedContext {
                name,
                context: Context {
                    cluster,
                    user,
                    namespace,
                    extras: Extras::new(),
                },
                extras: Extras::new(),
            });
            Ok(())
        })
    }

    pub fn update_context(
        &mut self,
        name: &str,
        cluster: &str,
        user: &str,
        namespace: Option<String>,
    ) -> Result<()> {
        let path = self
            .find_context(name)
            .map(|(p, _)| p.to_path_buf())
            .ok_or_else(|| crate::error::Error::NotFound {
                kind: "context",
                name: name.into(),
            })?;
        if self.find_cluster(cluster).is_none() {
            return Err(crate::error::Error::NotFound {
                kind: "cluster",
                name: cluster.into(),
            });
        }
        if self.find_user(user).is_none() {
            return Err(crate::error::Error::NotFound {
                kind: "user",
                name: user.into(),
            });
        }
        let (name, cluster, user) = (name.to_string(), cluster.to_string(), user.to_string());
        self.mutate(&path, move |cfg| {
            let entry = cfg
                .contexts
                .iter_mut()
                .rev()
                .find(|c| c.name == name)
                .ok_or_else(|| crate::error::Error::NotFound {
                    kind: "context",
                    name: name.clone(),
                })?;
            entry.context.cluster = cluster;
            entry.context.user = user;
            entry.context.namespace = namespace;
            Ok(())
        })
    }

    pub fn rename_context(&mut self, old: &str, new: &str) -> Result<()> {
        if self.find_context(new).is_some() {
            return Err(crate::error::Error::AlreadyExists {
                kind: "context",
                name: new.into(),
            });
        }
        let path = self
            .find_context(old)
            .map(|(p, _)| p.to_path_buf())
            .ok_or_else(|| crate::error::Error::NotFound {
                kind: "context",
                name: old.into(),
            })?;
        let (old, new) = (old.to_string(), new.to_string());
        self.mutate(&path, move |cfg| {
            let entry = cfg
                .contexts
                .iter_mut()
                .rev()
                .find(|c| c.name == old)
                .ok_or_else(|| crate::error::Error::NotFound {
                    kind: "context",
                    name: old.clone(),
                })?;
            entry.name = new.clone();
            if cfg.current_context.as_deref() == Some(old.as_str()) {
                cfg.current_context = Some(new);
            }
            Ok(())
        })
    }

    /// Removes only the context entry. Cluster/user entries are never cascaded.
    pub fn delete_context(&mut self, name: &str) -> Result<()> {
        let path = self
            .find_context(name)
            .map(|(p, _)| p.to_path_buf())
            .ok_or_else(|| crate::error::Error::NotFound {
                kind: "context",
                name: name.into(),
            })?;
        let name = name.to_string();
        self.mutate(&path, move |cfg| {
            cfg.contexts.retain(|c| c.name != name);
            if cfg.current_context.as_deref() == Some(name.as_str()) {
                cfg.current_context = None;
            }
            Ok(())
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

    fn store_ab(dir: &tempfile::TempDir) -> (KubeconfigStore, PathBuf, PathBuf) {
        let a = write(dir, "a.yaml", FILE_A);
        let b = write(dir, "b.yaml", FILE_B);
        let store = KubeconfigStore::load(vec![a.clone(), b.clone()]).unwrap();
        (store, a, b)
    }

    #[test]
    fn upsert_cluster_updates_in_place_preserving_extras() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, a, _) = store_ab(&dir);
        let new = Cluster {
            server: Some("https://new".into()),
            ..Default::default()
        };
        store.upsert_cluster("prod", new, None).unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(text.contains("https://new"));
        assert!(
            text.contains("my-extra"),
            "extras must survive edits: {text}"
        );
        assert_eq!(
            store
                .find_cluster("prod")
                .unwrap()
                .1
                .cluster
                .server
                .as_deref(),
            Some("https://new")
        );
    }

    #[test]
    fn upsert_cluster_inserts_new_into_target() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _, b) = store_ab(&dir);
        let c = Cluster {
            server: Some("https://c".into()),
            ..Default::default()
        };
        store.upsert_cluster("brand-new", c, Some(&b)).unwrap();
        let text = std::fs::read_to_string(&b).unwrap();
        assert!(text.contains("brand-new"));
        assert_eq!(store.find_cluster("brand-new").unwrap().0, b.as_path());
    }

    #[test]
    fn create_context_validates_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, a, _) = store_ab(&dir);
        let err = store
            .create_context("prod", "prod", "u1", None, None)
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::AlreadyExists { .. }));
        let err = store
            .create_context("x", "nope", "u1", None, None)
            .unwrap_err();
        assert!(matches!(err, crate::error::Error::NotFound { .. }));
        store
            .create_context("staging", "dev", "u1", Some("web".into()), None)
            .unwrap();
        let ctxs = store.contexts();
        let staging = ctxs.iter().find(|c| c.name == "staging").unwrap();
        assert_eq!(staging.source, a);
        assert_eq!(staging.namespace.as_deref(), Some("web"));
    }

    #[test]
    fn create_context_into_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _, _) = store_ab(&dir);
        let fresh = dir.path().join("fresh.yaml");
        store
            .create_context("iso", "dev", "u2", None, Some(&fresh))
            .unwrap();
        assert!(fresh.exists());
        assert_eq!(store.find_context("iso").unwrap().0, fresh.as_path());
    }

    #[test]
    fn rename_context_updates_current_context() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, a, _) = store_ab(&dir);
        let err = store.rename_context("prod", "dev").unwrap_err();
        assert!(matches!(err, crate::error::Error::AlreadyExists { .. }));
        store.rename_context("prod", "production").unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(text.contains("current-context: production"));
        assert!(store.find_context("production").is_some());
        assert!(
            store.find_context("prod").is_some(),
            "file B's prod context is now unmasked"
        );
    }

    #[test]
    fn update_context_rebinds() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _, _) = store_ab(&dir);
        store.update_context("prod", "dev", "u2", None).unwrap();
        let (_, ctx) = store.find_context("prod").unwrap();
        assert_eq!(ctx.context.cluster, "dev");
        assert_eq!(ctx.context.user, "u2");
        assert_eq!(ctx.context.namespace, None);
    }

    #[test]
    fn delete_context_no_cascade_clears_current() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, a, _) = store_ab(&dir);
        store.delete_context("prod").unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(!text.contains("current-context"));
        assert!(
            text.contains("name: prod"),
            "cluster/user entries must remain: {text}"
        );
        assert!(store.find_context("prod").is_some());
    }

    #[test]
    fn mutation_does_not_clobber_external_edits() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, a, _) = store_ab(&dir);
        let mut on_disk = crate::kubeconfig::io::read_file(&a).unwrap();
        on_disk.config.clusters.push(NamedCluster {
            name: "external".into(),
            cluster: Cluster {
                server: Some("https://ext".into()),
                ..Default::default()
            },
            extras: Extras::new(),
        });
        crate::kubeconfig::io::write_file(&on_disk).unwrap();
        store.delete_context("prod").unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(text.contains("external"), "external edit clobbered: {text}");
    }
}
