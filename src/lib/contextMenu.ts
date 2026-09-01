import type { ResourceKind } from "./api";

export const SCALABLE_KINDS = ["Deployment", "StatefulSet", "ReplicaSet", "ReplicationController"];
export const RESTARTABLE_KINDS = ["Deployment", "StatefulSet", "DaemonSet"];
/** Kinds whose `.spec.selector` lets us find and tail their pods. */
export const POD_OWNER_KINDS = [
  "Deployment",
  "StatefulSet",
  "DaemonSet",
  "ReplicaSet",
  "ReplicationController",
  "Job",
];
/** Kinds whose pods are found by name-prefix filtering the pods view. */
export const VIEW_PODS_KINDS = [...POD_OWNER_KINDS, "CronJob"];

/** Ids match ClusterActions method names so the menu dispatches straight to them. */
export type MenuActionId =
  | "describe"
  | "yaml"
  | "edit"
  | "copyName"
  | "copyYaml"
  | "logs"
  | "logsAll"
  | "shell"
  | "forward"
  | "scale"
  | "restart"
  | "history"
  | "cordon"
  | "uncordon"
  | "drain"
  | "trigger"
  | "suspend"
  | "resume"
  | "viewPods"
  | "values"
  | "del";

export interface MenuItem {
  id: MenuActionId;
  label: string;
  /** Keyboard shortcut hint shown on the right edge of the menu row. */
  key?: string;
  danger?: boolean;
}

/** Context-menu items for a row of the given kind, grouped for separators:
 * inspect (all kinds), then kind-specific operations, then delete. */
export function menuItemsFor(k: ResourceKind): MenuItem[][] {
  const core = k.group === "";
  const groups: MenuItem[][] = [
    [
      { id: "describe", label: "Describe", key: "d" },
      { id: "yaml", label: "View YAML", key: "y" },
      { id: "edit", label: "Edit YAML", key: "e" },
      { id: "copyName", label: "Copy name" },
      { id: "copyYaml", label: "Copy YAML" },
    ],
  ];

  const ops: MenuItem[] = [];
  if (core && k.kind === "Pod") {
    ops.push(
      { id: "logs", label: "Logs", key: "l" },
      { id: "shell", label: "Shell", key: "x" },
      { id: "forward", label: "Port-forward…", key: "f" },
    );
  }
  if (k.kind === "CronJob") {
    ops.push(
      { id: "trigger", label: "Trigger now" },
      { id: "suspend", label: "Suspend" },
      { id: "resume", label: "Resume" },
    );
  }
  if (POD_OWNER_KINDS.includes(k.kind)) ops.push({ id: "logsAll", label: "Logs (all pods)", key: "l" });
  if (VIEW_PODS_KINDS.includes(k.kind)) ops.push({ id: "viewPods", label: "View pods" });
  if (SCALABLE_KINDS.includes(k.kind)) ops.push({ id: "scale", label: "Scale…", key: "s" });
  if (RESTARTABLE_KINDS.includes(k.kind))
    ops.push({ id: "restart", label: "Restart rollout", key: "r" });
  if (k.kind === "Deployment") ops.push({ id: "history", label: "Rollout history…" });
  if (core && k.kind === "Node") {
    ops.push(
      { id: "cordon", label: "Cordon", key: "c" },
      { id: "uncordon", label: "Uncordon", key: "u" },
      { id: "drain", label: "Drain…", danger: true },
    );
  }
  if (core && k.kind === "Service") ops.push({ id: "forward", label: "Port-forward…", key: "f" });
  if (core && (k.kind === "Secret" || k.kind === "ConfigMap"))
    ops.push({ id: "values", label: "Copy value…" });
  if (ops.length > 0) groups.push(ops);

  groups.push([{ id: "del", label: "Delete…", key: "ctrl d", danger: true }]);
  return groups;
}
