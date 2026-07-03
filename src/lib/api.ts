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
};
