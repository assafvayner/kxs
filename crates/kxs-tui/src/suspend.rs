//! Mutations and terminal-suspend actions, dispatched to kxs-cluster.

use k8s_openapi::chrono::Utc;
use kxs_cluster::edit::{
    apply_edit, cordon_patch, delete_resource, merge_patch, restart_patch, scale_patch,
    suspend_patch,
};

use crate::cmd::{Mutation, SuspendAction};

/// Dispatch a mutation to its kxs-cluster call. `Ok(Some(text))` surfaces a
/// flash message (e.g. the created Job name).
pub async fn mutate(client: kube::Client, m: Mutation) -> Result<Option<String>, String> {
    match m {
        Mutation::Scale {
            kind,
            ns,
            name,
            replicas,
        } => {
            merge_patch(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                Some(&ns),
                &name,
                scale_patch(replicas),
            )
            .await?;
            Ok(Some(format!("scaled {name} to {replicas}")))
        }
        Mutation::Restart { kind, ns, name } => {
            let now = Utc::now().to_rfc3339();
            merge_patch(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                Some(&ns),
                &name,
                restart_patch(&now),
            )
            .await?;
            Ok(Some(format!("restart rolled out for {name}")))
        }
        Mutation::Cordon {
            name,
            unschedulable,
            ..
        } => {
            merge_patch(
                client,
                "",
                "v1",
                "Node",
                "nodes",
                None,
                &name,
                cordon_patch(unschedulable),
            )
            .await?;
            Ok(Some(if unschedulable {
                format!("cordoned {name}")
            } else {
                format!("uncordoned {name}")
            }))
        }
        Mutation::Drain { name, .. } => {
            let report = kxs_cluster::workloads::drain_node(client, &name).await?;
            Ok(Some(format!(
                "drained {name}: evicted {} skipped {} failed {}",
                report.evicted,
                report.skipped,
                report.failed.len()
            )))
        }
        Mutation::Trigger { ns, name } => {
            let job = kxs_cluster::workloads::trigger_cronjob(client, &ns, &name).await?;
            Ok(Some(format!("created job {job}")))
        }
        Mutation::Suspend { ns, name, suspend } => {
            merge_patch(
                client,
                "batch",
                "v1",
                "CronJob",
                "cronjobs",
                Some(&ns),
                &name,
                suspend_patch(suspend),
            )
            .await?;
            Ok(Some(if suspend {
                format!("suspended {name}")
            } else {
                format!("resumed {name}")
            }))
        }
        Mutation::Undo { ns, name, revision } => {
            kxs_cluster::workloads::rollout_undo(client, &ns, &name, revision).await?;
            Ok(Some(format!("rolled back {name} to revision {revision}")))
        }
        Mutation::Delete {
            kind,
            ns,
            name,
            propagation,
            force,
        } => {
            let ns = if kind.namespaced {
                Some(ns.as_str())
            } else {
                None
            };
            delete_resource(
                client,
                &kind.group,
                &kind.version,
                &kind.kind,
                &kind.plural,
                ns,
                &name,
                propagation.as_deref(),
                force,
            )
            .await?;
            Ok(Some(format!("deleted {}/{}", kind.kind, name)))
        }
    }
}

/// The `e` edit flow, run with the TUI suspended: fetch YAML, write a temp
/// file, hand it to $KUBE_EDITOR/$EDITOR/vi, apply on save. Server errors are
/// prepended as `# ` comments and the editor reopens (kubectl behavior).
/// `Ok(None)` means "no changes".
pub async fn run_edit(
    client: kube::Client,
    action: SuspendAction,
) -> Result<Option<String>, String> {
    let SuspendAction::Edit { kind, ns, name } = action;
    let yaml = kxs_cluster::resources::get_yaml(
        client.clone(),
        &kind.group,
        &kind.version,
        &kind.kind,
        &kind.plural,
        ns.as_deref(),
        &name,
    )
    .await?;
    let path = std::env::temp_dir().join(format!(
        "kxs-{}-{}-{}.yaml",
        kind.plural,
        name,
        std::process::id()
    ));
    let mut current = yaml.clone();
    let outcome = loop {
        write_secret_file(&path, &current)?;
        let editor = std::env::var("KUBE_EDITOR")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".into());
        // shell form so editors with arguments ("code -w") work
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$0\""))
            .arg(&path)
            .status()
            .map_err(|e| format!("cannot run editor {editor}: {e}"))?;
        if !status.success() {
            return Err(format!("editor exited with {status}; aborting edit"));
        }
        let edited = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if edited == current {
            break Ok(None); // unchanged: abort
        }
        let result = apply_edit(
            client.clone(),
            &kind.group,
            &kind.version,
            &kind.kind,
            &kind.plural,
            ns.as_deref(),
            &name,
            &yaml,
            &edited,
            false,
        )
        .await;
        match result {
            Ok(_) => break Ok(Some(format!("edited {}/{}", kind.kind, name))),
            Err(e) => {
                // prepend the error as a comment and reopen the editor
                current = format!(
                    "# edit failed, fix or save unchanged to abort:\n# {}\n\n{}",
                    e.replace('\n', "\n# "),
                    edited
                );
            }
        }
    };
    let _ = std::fs::remove_file(&path);
    outcome
}

fn write_secret_file(path: &std::path::Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    f.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    f.write_all(content.as_bytes()).map_err(|e| e.to_string())
}
