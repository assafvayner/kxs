import type { ResourceKind } from "./api";
import type { View } from "./stores/viewstack.svelte";

export function resolveKind(kinds: ResourceKind[], query: string): ResourceKind | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  return kinds.find((k) => k.aliases.includes(q) || k.kind.toLowerCase() === q || k.plural === q) ?? null;
}

/** Kinds visible in the picker: all when unprobed; else cluster-scoped + present namespaced. */
export function visibleKinds(kinds: ResourceKind[], present: Set<string> | null): ResourceKind[] {
  if (present === null) return kinds;
  return kinds.filter((k) => !k.namespaced || present.has(k.group + "/" + k.kind));
}

export function fuzzyKinds(kinds: ResourceKind[], query: string): ResourceKind[] {
  const q = query.trim().toLowerCase();
  if (!q) return kinds;
  const score = (k: ResourceKind): number => {
    if (k.aliases.includes(q)) return 0;
    if (k.kind.toLowerCase().startsWith(q) || k.plural.startsWith(q)) return 1;
    if (k.kind.toLowerCase().includes(q) || k.aliases.some((a) => a.includes(q))) return 2;
    return 99;
  };
  return kinds
    .map((k) => [k, score(k)] as const)
    .filter(([, s]) => s < 99)
    .sort((a, b) => a[1] - b[1] || a[0].kind.localeCompare(b[0].kind))
    .map(([k]) => k);
}

/** Substring by default; `-r <regex>` for regex. Invalid regex → no match. */
export function matchRow(name: string, filter: string): boolean {
  const f = filter.trim();
  if (!f) return true;
  if (f.startsWith("-r ")) {
    try {
      return new RegExp(f.slice(3)).test(name);
    } catch {
      return false;
    }
  }
  return name.toLowerCase().includes(f.toLowerCase());
}

/** Label for the resource switcher: nearest pods/resource view kind from the top, else "Pods". */
export function currentKindLabel(views: View[]): string {
  for (let i = views.length - 1; i >= 0; i--) {
    const v = views[i];
    if (v.kind === "pods") return "Pods";
    if (v.kind === "resource") return v.resourceKind.kind;
  }
  return "Pods";
}

/** The search bar is active for every view except the exec terminal. */
export function searchEnabled(view: View): boolean {
  return view.kind !== "exec";
}
