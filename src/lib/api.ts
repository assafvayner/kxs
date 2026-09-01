import { Channel, invoke } from "@tauri-apps/api/core";

export interface ContextSummary {
  name: string;
  cluster: string;
  user: string;
  namespace: string | null;
  source: string;
}

export interface KubeconfigView {
  contexts: ContextSummary[];
  currentContext: string | null;
  files: string[];
  defaultTarget: string;
  warnings: string[];
}

export interface ContextDetail {
  name: string;
  namespace: string | null;
  source: string;
  clusterName: string;
  server: string | null;
  caFile: string | null;
  caData: string | null;
  insecureSkipTlsVerify: boolean;
  userName: string;
  token: string | null;
  clientCertificate: string | null;
  clientKey: string | null;
  clientCertificateData: string | null;
  clientKeyData: string | null;
  execCommand: string | null;
  execArgs: string[];
  execEnv: [string, string][];
  execApiVersion: string | null;
}

export interface ClusterSpec {
  existing?: string;
  name?: string;
  server?: string;
  caFile?: string;
  caData?: string;
  insecureSkipTlsVerify?: boolean;
}

export interface UserSpec {
  existing?: string;
  name?: string;
  token?: string;
  clientCertificate?: string;
  clientKey?: string;
  clientCertificateData?: string;
  clientKeyData?: string;
  execCommand?: string;
  execArgs?: string[];
  execEnv?: [string, string][];
  execApiVersion?: string;
}

export interface ContextSpec {
  name: string;
  originalName?: string;
  namespace?: string;
  targetFile?: string;
  cluster: ClusterSpec;
  user: UserSpec;
}

export interface SessionInfo {
  version: string;
  defaultNamespace: string;
}

export interface PodRow {
  key: string;
  name: string;
  namespace: string;
  ready: string;
  status: string;
  restarts: number;
  ip: string | null;
  node: string | null;
  created: string | null;
}

export type PodEvent =
  | { type: "snapshot"; rows: PodRow[] }
  | { type: "upsert"; rows: PodRow[] }
  | { type: "delete"; keys: string[] }
  | { type: "status"; state: string; message: string | null };

export interface LogRequest {
  namespace: string;
  pod: string;
  container?: string;
  follow: boolean;
  tailLines?: number;
  sinceSeconds?: number;
  timestamps: boolean;
  previous: boolean;
}

export type LogEvent =
  | { type: "lines"; lines: string[] }
  | { type: "error"; message: string }
  | { type: "eof" };

export interface ContainerPortInfo {
  name: string | null;
  containerPort: number;
}

export interface ContainerInfo {
  name: string;
  image: string;
  ready: boolean;
  restarts: number;
  ports: ContainerPortInfo[];
  initContainer: boolean;
}

export interface ResourceKind {
  group: string;
  version: string;
  kind: string;
  plural: string;
  namespaced: boolean;
  aliases: string[];
}
export interface ResourceRow {
  key: string;
  name: string;
  namespace: string | null;
  cells: string[];
  created: string | null;
}
export interface ResourceTable {
  columns: string[];
  rows: ResourceRow[];
}
export type ResourceTableEvent =
  | { type: "table"; table: ResourceTable }
  | { type: "status"; state: string; message: string | null };
export interface ResourceEvent {
  type: string;
  reason: string;
  message: string;
  count: number;
  lastSeen: string | null;
}

export type ExecEvent =
  | { type: "output"; data: string }
  | { type: "closed"; message: string | null };

export interface ForwardInfo {
  id: number;
  localPort: number;
  namespace: string;
  pod: string;
  podPort: number;
}

export interface RolloutRevision {
  revision: number;
  name: string;
  created: string | null;
  images: string[];
  replicas: number;
  current: boolean;
}

export interface DrainReport {
  evicted: number;
  skipped: number;
  failed: string[];
}

export interface ConfigEntry {
  key: string;
  value: string;
  binary: boolean;
}

export interface MetricsRow {
  key: string;
  name: string;
  namespace: string | null;
  cpuMillicores: number;
  memMib: number;
}

export type PropagationPolicy = "Background" | "Foreground" | "Orphan";

export interface DeleteOptions {
  /** Omitted → the server's per-resource default garbage-collection policy. */
  propagation?: PropagationPolicy | null;
  /** Grace period 0, for objects stuck terminating. */
  force?: boolean;
}

export const api = {
  listContexts: () => invoke<KubeconfigView>("list_contexts"),
  getContext: (name: string) => invoke<ContextDetail>("get_context", { name }),
  saveContext: (spec: ContextSpec) => invoke<void>("save_context", { spec }),
  deleteContext: (name: string) => invoke<void>("delete_context", { name }),
  pingContext: (context: string) => invoke<string>("ping_context", { context }),
  openSession: (tabId: number, context: string) =>
    invoke<SessionInfo>("open_session", { tabId, context }),
  closeSession: (tabId: number) => invoke<void>("close_session", { tabId }),
  listNamespaces: (tabId: number) => invoke<string[]>("list_namespaces", { tabId }),
  watchPods: (tabId: number, namespace: string | null, onEvent: (e: PodEvent) => void) => {
    const channel = new Channel<PodEvent>();
    channel.onmessage = onEvent;
    return invoke<void>("watch_pods", { tabId, namespace, channel });
  },
  listContainers: (tabId: number, namespace: string, pod: string) =>
    invoke<string[]>("list_containers", { tabId, namespace, pod }),
  listContainerInfo: (tabId: number, namespace: string, pod: string) =>
    invoke<ContainerInfo[]>("list_container_info", { tabId, namespace, pod }),
  streamLogs: (tabId: number, request: LogRequest, onEvent: (e: LogEvent) => void) => {
    const channel = new Channel<LogEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("stream_logs", { tabId, request, channel });
  },
  stopLogs: (tabId: number, streamId: number) =>
    invoke<void>("stop_logs", { tabId, streamId }),
  listResourceKinds: (tabId: number) =>
    invoke<ResourceKind[]>("list_resource_kinds", { tabId }),
  listPresentKinds: (tabId: number, namespace: string | null, kinds: ResourceKind[]) =>
    invoke<string[]>("list_present_kinds", { tabId, namespace, kinds }),
  listResourceTable: (
    tabId: number,
    group: string,
    version: string,
    plural: string,
    namespace: string | null,
  ) => invoke<ResourceTable>("list_resource_table", { tabId, group, version, plural, namespace }),
  watchResourceTable: (
    tabId: number,
    k: ResourceKind,
    namespace: string | null,
    onEvent: (e: ResourceTableEvent) => void,
  ) => {
    const channel = new Channel<ResourceTableEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("watch_resource_table", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      channel,
    });
  },
  stopResourceTable: (tabId: number, watchId: number) =>
    invoke<void>("stop_resource_table", { tabId, watchId }),
  getResourceYaml: (tabId: number, k: ResourceKind, namespace: string | null, name: string) =>
    invoke<string>("get_resource_yaml", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
    }),
  getResourceEvents: (tabId: number, namespace: string | null, kind: string, name: string) =>
    invoke<ResourceEvent[]>("get_resource_events", { tabId, namespace, kind, name }),
  applyYaml: (
    tabId: number,
    k: ResourceKind,
    namespace: string | null,
    name: string,
    yaml: string,
    dryRun: boolean,
  ) =>
    invoke<void>("apply_resource_yaml", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
      yaml,
      dryRun,
    }),
  deleteResource: (
    tabId: number,
    k: ResourceKind,
    namespace: string | null,
    name: string,
    opts: DeleteOptions = {},
  ) =>
    invoke<void>("delete_resource", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
      propagation: opts.propagation ?? null,
      force: opts.force ?? false,
    }),
  scaleResource: (
    tabId: number,
    k: ResourceKind,
    namespace: string | null,
    name: string,
    replicas: number,
  ) =>
    invoke<void>("scale_resource", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
      replicas,
    }),
  restartResource: (
    tabId: number,
    k: ResourceKind,
    namespace: string | null,
    name: string,
    restartedAt: string,
  ) =>
    invoke<void>("restart_resource", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
      restartedAt,
    }),
  cordonNode: (tabId: number, name: string, unschedulable: boolean) =>
    invoke<void>("cordon_node", { tabId, name, unschedulable }),
  startExec: (
    tabId: number,
    namespace: string,
    pod: string,
    container: string | null,
    command: string[],
    cols: number,
    rows: number,
    onEvent: (e: ExecEvent) => void,
  ) => {
    const channel = new Channel<ExecEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("start_exec", {
      tabId,
      namespace,
      pod,
      container,
      command,
      cols,
      rows,
      channel,
    });
  },
  execStdin: (tabId: number, execId: number, dataB64: string) =>
    invoke<void>("exec_stdin", { tabId, execId, data: dataB64 }),
  execResize: (tabId: number, execId: number, cols: number, rows: number) =>
    invoke<void>("exec_resize", { tabId, execId, cols, rows }),
  stopExec: (tabId: number, execId: number) => invoke<void>("stop_exec", { tabId, execId }),
  startForward: (tabId: number, namespace: string, pod: string, podPort: number) =>
    invoke<ForwardInfo>("start_forward", { tabId, namespace, pod, podPort }),
  forwardService: (tabId: number, namespace: string, service: string, servicePort: number) =>
    invoke<ForwardInfo>("forward_service", { tabId, namespace, service, servicePort }),
  stopForward: (tabId: number, forwardId: number) =>
    invoke<void>("stop_forward", { tabId, forwardId }),
  listForwards: (tabId: number) => invoke<ForwardInfo[]>("list_forwards", { tabId }),
  podMetrics: (tabId: number, namespace: string | null) =>
    invoke<MetricsRow[]>("pod_metrics", { tabId, namespace }),
  listWorkloadPods: (tabId: number, k: ResourceKind, namespace: string, name: string) =>
    invoke<string[]>("list_workload_pods", {
      tabId,
      group: k.group,
      version: k.version,
      kind: k.kind,
      plural: k.plural,
      namespace,
      name,
    }),
  rolloutHistory: (tabId: number, namespace: string, name: string) =>
    invoke<RolloutRevision[]>("rollout_history", { tabId, namespace, name }),
  rolloutUndo: (tabId: number, namespace: string, name: string, revision: number) =>
    invoke<void>("rollout_undo", { tabId, namespace, name, revision }),
  drainNode: (tabId: number, name: string) => invoke<DrainReport>("drain_node", { tabId, name }),
  triggerCronjob: (tabId: number, namespace: string, name: string) =>
    invoke<string>("trigger_cronjob", { tabId, namespace, name }),
  suspendCronjob: (tabId: number, namespace: string, name: string, suspend: boolean) =>
    invoke<void>("suspend_cronjob", { tabId, namespace, name, suspend }),
  getConfigValues: (tabId: number, namespace: string, name: string, kind: string) =>
    invoke<ConfigEntry[]>("get_config_values", { tabId, namespace, name, kind }),
};
