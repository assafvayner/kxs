import type { ResourceKind } from "./api";
import type { View } from "./stores/viewstack.svelte";

export const VIEW_MEMORY_VERSION = 1;
const KEY_PREFIX = "kxs.viewmemory.";

/**
 * Stable identity of a resource kind. The served version drifts between
 * clusters and upgrades, so it is deliberately not stored; the kind is
 * resolved against live discovery on restore.
 */
export interface RememberedResource {
  group: string;
  kind: string;
  plural: string;
}

export interface ViewMemory {
  v: typeof VIEW_MEMORY_VERSION;
  /** null = all namespaces */
  namespace: string | null;
  /** null = the pods view */
  resource: RememberedResource | null;
  filter: string;
}

export function viewMemoryKey(context: string): string {
  return KEY_PREFIX + context;
}

export function rememberedResourceOf(k: ResourceKind): RememberedResource {
  return { group: k.group, kind: k.kind, plural: k.plural };
}

/**
 * Nearest top-level view looking down from the top of the stack; null = pods.
 * Drill-ins (yaml, logs, exec, …) are transparent: they reference objects that
 * may be gone by the next launch, so they never become the remembered view.
 */
export function topLevelKind(views: readonly View[]): ResourceKind | null {
  for (let i = views.length - 1; i >= 0; i--) {
    const v = views[i];
    if (v.kind === "pods") return null;
    if (v.kind === "resource") return v.resourceKind;
  }
  return null;
}

export function viewMemoryOf(
  namespace: string | null,
  views: readonly View[],
  filter: string,
): ViewMemory {
  const k = topLevelKind(views);
  return {
    v: VIEW_MEMORY_VERSION,
    namespace,
    resource: k === null ? null : rememberedResourceOf(k),
    filter,
  };
}

function parseResource(raw: unknown): RememberedResource | null {
  if (raw === null || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.group !== "string") return null;
  if (typeof r.kind !== "string" || !r.kind) return null;
  if (typeof r.plural !== "string" || !r.plural) return null;
  return { group: r.group, kind: r.kind, plural: r.plural };
}

/** Corrupt, empty or stale-version payloads yield null → caller uses defaults. */
export function parseViewMemory(raw: string | null): ViewMemory | null {
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (parsed === null || typeof parsed !== "object") return null;
  const m = parsed as Record<string, unknown>;
  if (m.v !== VIEW_MEMORY_VERSION) return null;
  const ns = m.namespace;
  if (ns !== null && (typeof ns !== "string" || !ns)) return null;
  return {
    v: VIEW_MEMORY_VERSION,
    namespace: ns,
    resource: parseResource(m.resource ?? null),
    filter: typeof m.filter === "string" ? m.filter : "",
  };
}

export function serializeViewMemory(m: ViewMemory): string {
  return JSON.stringify(m);
}

/** The live kind carrying the remembered identity: group+kind, else group+plural. */
export function resolveRememberedResource(
  kinds: readonly ResourceKind[],
  r: RememberedResource,
): ResourceKind | null {
  return (
    kinds.find((k) => k.group === r.group && k.kind === r.kind) ??
    kinds.find((k) => k.group === r.group && k.plural === r.plural) ??
    null
  );
}

/** All-namespaces is always available; a named one must still exist. */
export function namespaceAvailable(
  namespaces: readonly string[],
  namespace: string | null,
): boolean {
  return namespace === null || namespaces.includes(namespace);
}

export function loadViewMemory(context: string): ViewMemory | null {
  try {
    if (typeof localStorage === "undefined") return null;
    return parseViewMemory(localStorage.getItem(viewMemoryKey(context)));
  } catch {
    return null;
  }
}

export function saveViewMemory(context: string, m: ViewMemory): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(viewMemoryKey(context), serializeViewMemory(m));
  } catch {
    /* persistence is best-effort */
  }
}
