import { SvelteMap } from "svelte/reactivity";
import type { ResourceKind } from "../api";
import { PodTable } from "./podtable.svelte";
import { ViewStack } from "./viewstack.svelte";

export type SessionStatus = "connecting" | "ready" | "error";

export class TabSession {
  status = $state<SessionStatus>("connecting");
  error = $state<string | null>(null);
  version = $state("");
  namespaces = $state<string[]>([]);
  /** null = all namespaces */
  namespace = $state<string | null>(null);
  watchState = $state<"starting" | "live" | "reconnecting">("starting");
  pods = new PodTable();
  views = new ViewStack();
  kinds = $state<ResourceKind[]>([]);
  filter = $state("");
  selected = $state<string | null>(null);
}

/** tabId → session UI state; TabBar reads dots from here. */
export const sessions = new SvelteMap<number, TabSession>();
