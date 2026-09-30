//! Kind capability lists, ported from `src/lib/contextMenu.ts`
//! (`menuItemsFor` itself is DOM-bound and stays in the desktop app).

pub const SCALABLE_KINDS: [&str; 4] = [
    "Deployment",
    "StatefulSet",
    "ReplicaSet",
    "ReplicationController",
];
pub const RESTARTABLE_KINDS: [&str; 3] = ["Deployment", "StatefulSet", "DaemonSet"];
/// Kinds whose `.spec.selector` lets us find and tail their pods.
pub const POD_OWNER_KINDS: [&str; 6] = [
    "Deployment",
    "StatefulSet",
    "DaemonSet",
    "ReplicaSet",
    "ReplicationController",
    "Job",
];
/// Kinds whose pods are found by name-prefix filtering the pods view.
pub const VIEW_PODS_KINDS: [&str; 7] = [
    "Deployment",
    "StatefulSet",
    "DaemonSet",
    "ReplicaSet",
    "ReplicationController",
    "Job",
    "CronJob",
];

pub fn is_scalable(kind: &str) -> bool {
    SCALABLE_KINDS.contains(&kind)
}

pub fn is_restartable(kind: &str) -> bool {
    RESTARTABLE_KINDS.contains(&kind)
}

pub fn is_pod_owner(kind: &str) -> bool {
    POD_OWNER_KINDS.contains(&kind)
}

pub fn views_pods(kind: &str) -> bool {
    VIEW_PODS_KINDS.contains(&kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership() {
        assert!(is_scalable("Deployment"));
        assert!(!is_scalable("DaemonSet"));
        assert!(is_restartable("StatefulSet"));
        assert!(!is_restartable("ReplicaSet"));
        assert!(is_pod_owner("Job"));
        assert!(!is_pod_owner("CronJob"));
        assert!(views_pods("CronJob"));
        assert!(!views_pods("Pod"));
    }
}
