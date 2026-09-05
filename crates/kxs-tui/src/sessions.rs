//! Session and kubeconfig state for the TUI, shared between `App` and
//! `Runtime` behind a mutex.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use kxs_cluster::discovery::ResourceKind;
use kxs_cluster::session::ClusterSession;
use kxs_core::kubeconfig::store::KubeconfigStore;

/// The currently connected context.
#[derive(Debug, Clone)]
pub struct ActiveContext {
    pub name: String,
    /// `None` = all namespaces.
    pub namespace: Option<String>,
    /// Version string from the connect ping, shown in the header.
    pub version: String,
}

impl Default for Sessions {
    fn default() -> Self {
        Sessions {
            store: KubeconfigStore::load_tolerant(vec![]).0,
            map: Default::default(),
            kinds: Default::default(),
            present: Default::default(),
            active: None,
            forwards: Default::default(),
            next_forward_id: 0,
        }
    }
}

pub struct Sessions {
    pub store: KubeconfigStore,
    pub map: HashMap<String, Arc<ClusterSession>>,
    /// Discovery per context, cached on connect.
    pub kinds: HashMap<String, Arc<Vec<ResourceKind>>>,
    /// `present_kinds` probe per context (`group/kind` keys), cached and
    /// refreshed every 5 minutes.
    pub present: HashMap<String, Arc<HashSet<String>>>,
    pub active: Option<ActiveContext>,
    /// Port-forwards, owned by the session (not by a view), so they survive
    /// navigation and die on quit (senders dropped → proxy tasks end).
    pub forwards: Vec<Forward>,
    next_forward_id: u64,
}

/// One live port-forward.
pub struct Forward {
    pub id: u64,
    pub ns: String,
    pub pod: String,
    pub container: Option<String>,
    pub pod_port: u16,
    pub local_port: u16,
    pub started: std::time::Instant,
    pub stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Forward {
    /// Stops the proxy task; idempotent.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
    }
}

impl Sessions {
    pub fn new(store: KubeconfigStore) -> Self {
        Sessions {
            store,
            map: Default::default(),
            kinds: Default::default(),
            present: Default::default(),
            active: None,
            forwards: Default::default(),
            next_forward_id: 0,
        }
    }

    pub fn add_forward(&mut self, f: Forward) {
        self.forwards.push(f);
    }

    pub fn next_forward_id(&mut self) -> u64 {
        self.next_forward_id += 1;
        self.next_forward_id
    }

    pub fn stop_forward(&mut self, id: u64) {
        if let Some(f) = self.forwards.iter_mut().find(|f| f.id == id) {
            f.stop();
        }
        self.forwards.retain(|f| f.id != id);
    }

    pub fn active_session(&self) -> Option<Arc<ClusterSession>> {
        let name = self.active.as_ref()?.name.clone();
        self.map.get(&name).cloned()
    }

    pub fn active_kinds(&self) -> Arc<Vec<ResourceKind>> {
        match &self.active {
            Some(a) => self.kinds.get(&a.name).cloned().unwrap_or_default(),
            None => Arc::new(Vec::new()),
        }
    }

    pub fn active_present(&self) -> Option<Arc<HashSet<String>>> {
        let a = self.active.as_ref()?;
        self.present.get(&a.name).cloned()
    }
}

pub type Shared = Arc<Mutex<Sessions>>;
