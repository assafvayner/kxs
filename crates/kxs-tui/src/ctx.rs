//! Read-only snapshot handed to views for key handling and rendering.

use std::collections::HashSet;
use std::sync::Arc;

use kxs_cluster::discovery::ResourceKind;

pub struct AppCtx {
    /// Namespace of the active context; `None` = all.
    pub namespace: Option<String>,
    pub kinds: Arc<Vec<ResourceKind>>,
    pub present: Option<Arc<HashSet<String>>>,
    pub readonly: bool,
    /// (columns, rows) of the terminal.
    pub size: (u16, u16),
}
