import { linter, type Diagnostic } from "@codemirror/lint";
import { StateField, type Extension } from "@codemirror/state";
import { parseDocument } from "yaml";
import { schemaDiagnostics, unknownFieldDiagnostics } from "./diagnostics";
import { findRootPair, keyRange } from "./positions";
import type { SchemaProvider } from "./provider";
import { parseApiVersion, sameGvk, type Gvk } from "./schema";

/** apiVersion and kind from the head of the buffer; null when either is missing. */
export function gvkFromText(text: string): Gvk | null {
  const head = text.slice(0, 4096);
  const apiVersion = head.match(/^apiVersion:\s*([^\s#]+)/m);
  const kind = head.match(/^kind:\s*([^\s#]+)/m);
  if (!apiVersion || !kind) return null;
  return { ...parseApiVersion(apiVersion[1]), kind: kind[1] };
}

/** The document's current kind, falling back to the resource the editor opened when the header is unparseable. */
export function gvkField(fallback: Gvk): StateField<Gvk> {
  return StateField.define<Gvk>({
    create: (state) => gvkFromText(state.doc.toString()) ?? fallback,
    update(value, tr) {
      if (!tr.docChanged) return value;
      const next = gvkFromText(tr.newDoc.toString()) ?? value;
      return sameGvk(next, value) ? value : next;
    },
  });
}

export function k8sLint(provider: SchemaProvider, gvk: StateField<Gvk>): Extension {
  return linter((view) => lintDocument(view.state.doc.toString(), view.state.field(gvk), provider), {
    delay: 400,
    needsRefresh: (u) => u.startState.field(gvk) !== u.state.field(gvk),
  });
}

export async function lintDocument(text: string, gvk: Gvk, provider: SchemaProvider): Promise<Diagnostic[]> {
  const doc = parseDocument(text);
  const parse: Diagnostic[] = [
    ...doc.errors.map((e) => ({ from: e.pos[0], to: e.pos[1], severity: "error" as const, source: "yaml", message: parseMessage(e) })),
    ...doc.warnings.map((e) => ({ from: e.pos[0], to: e.pos[1], severity: "warning" as const, source: "yaml", message: parseMessage(e) })),
  ];
  if (doc.errors.length || doc.contents === null) return parse;

  let schema;
  try {
    schema = await provider.schemaFor(gvk);
  } catch {
    return parse;
  }
  if (!schema) {
    const r = keyRange(findRootPair(doc, "kind")) ?? { from: 0, to: 0 };
    const gv = gvk.group ? `${gvk.group}/${gvk.version}` : gvk.version;
    return [...parse, { ...r, severity: "info", source: "k8s-schema", message: `No schema for ${gv} ${gvk.kind} on this cluster` }];
  }
  const unknown = unknownFieldDiagnostics(doc, schema.root, schema.schemas);
  let value: unknown;
  try {
    value = doc.toJS();
  } catch {
    // alias expansion limits throw after a clean parse
    return [...parse, ...unknown];
  }
  return [...parse, ...schemaDiagnostics(doc, schema.validator.validate(value).errors), ...unknown];
}

function parseMessage(e: { code: string; message: string }): string {
  return e.code === "MULTIPLE_DOCS" ? "Only one document can be edited here; remove the extra `---` documents" : e.message;
}
