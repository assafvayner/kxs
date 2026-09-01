import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import type { StateField } from "@codemirror/state";
import { cursorContext, type CursorContext } from "./cursor";
import type { SchemaProvider } from "./provider";
import { deref, schemaAt, typeLabel, type Gvk, type SchemaMap, type SchemaNode } from "./schema";

const WORD = /^[A-Za-z0-9_./-]*$/;

export function k8sCompletion(provider: SchemaProvider, gvk: StateField<Gvk>) {
  return async (ctx: CompletionContext): Promise<CompletionResult | null> => {
    const cur = cursorContext(ctx.state.doc.toString(), ctx.pos);
    if (!cur) return null;
    if (!cur.word && !ctx.explicit) return null;
    const schema = await provider.schemaFor(ctx.state.field(gvk)).catch(() => null);
    if (!schema) return null;
    const options = completionOptions(cur, schema.root, schema.schemas);
    return options.length ? { from: cur.from, options, validFor: WORD } : null;
  };
}

export function completionOptions(cur: CursorContext, root: SchemaNode, schemas: SchemaMap): Completion[] {
  const container = schemaAt(root, schemas, cur.path);
  if (!container) return [];
  if (cur.mode === "key") {
    const pad = " ".repeat(cur.indent + 2);
    return Object.entries(container.properties ?? {})
      .filter(([name]) => !cur.siblings.includes(name))
      .map(([name, raw]) => {
        const s = deref(raw, schemas);
        const shape = s?.type === "array" ? "array" : s?.properties ? "object" : "scalar";
        const apply = shape === "object" ? `${name}:\n${pad}` : shape === "array" ? `${name}:\n${pad}- ` : `${name}: `;
        return {
          label: name,
          type: "property",
          detail: typeLabel(raw, schemas),
          info: s?.description,
          apply,
          boost: container.required?.includes(name) ? 1 : 0,
        };
      });
  }
  const s = deref(container.properties?.[cur.key ?? ""], schemas);
  if (!s) return [];
  if (s.enum) return s.enum.map((v) => ({ label: String(v), type: "enum" }));
  if (s.type === "boolean") return [{ label: "true", type: "keyword" }, { label: "false", type: "keyword" }];
  return [];
}
