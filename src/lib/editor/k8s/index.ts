import { autocompletion } from "@codemirror/autocomplete";
import { lintGutter } from "@codemirror/lint";
import type { Extension } from "@codemirror/state";
import { k8sCompletion } from "./complete";
import { gvkField, k8sLint } from "./lint";
import type { SchemaProvider } from "./provider";
import type { Gvk } from "./schema";

export interface K8sOptions {
  provider: SchemaProvider;
  /** Kind the editor was opened on, used until the buffer's own header parses. */
  fallback: Gvk;
}

export function k8sExtensions({ provider, fallback }: K8sOptions): Extension {
  const gvk = gvkField(fallback);
  return [
    gvk,
    k8sLint(provider, gvk),
    lintGutter(),
    autocompletion({ override: [k8sCompletion(provider, gvk)], icons: false }),
  ];
}
