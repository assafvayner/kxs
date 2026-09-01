import type { OutputUnit } from "@cfworker/json-schema";
import type { Diagnostic } from "@codemirror/lint";
import { isMap, isScalar, isSeq, type Document, type Pair, type ParsedNode } from "yaml";
import { keyRange, ownerKeyRange, valueRange, type Range } from "./positions";
import { deref, type SchemaMap, type SchemaNode } from "./schema";

export const SOURCE = "k8s-schema";

const LEAF_KEYWORDS = new Set([
  "type", "enum", "const", "required", "pattern", "format",
  "minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum",
  "minLength", "maxLength", "minItems", "maxItems", "uniqueItems", "multipleOf",
  "minProperties", "maxProperties",
]);

/** Validator output to diagnostics: leaf keywords only, one diagnostic per range. */
export function schemaDiagnostics(doc: Document, errors: OutputUnit[]): Diagnostic[] {
  const byRange = new Map<string, { range: Range; messages: string[] }>();
  for (const e of errors) {
    if (!LEAF_KEYWORDS.has(e.keyword)) continue;
    const range = e.keyword === "required" ? ownerKeyRange(doc, e.instanceLocation) : valueRange(doc, e.instanceLocation);
    if (!range) continue;
    const message = friendly(e);
    const key = `${range.from}:${range.to}`;
    const entry = byRange.get(key) ?? { range, messages: [] };
    if (!entry.messages.includes(message)) entry.messages.push(message);
    byRange.set(key, entry);
  }
  return [...byRange.values()].map(({ range, messages }) => ({
    from: range.from,
    to: range.to,
    severity: "error",
    source: SOURCE,
    message: mergeTypeMessages(messages),
  }));
}

function friendly(e: OutputUnit): string {
  let m: RegExpMatchArray | null;
  if (e.keyword === "required" && (m = e.error.match(/required property "([^"]+)"/))) {
    return `Missing required field "${m[1]}"`;
  }
  if (e.keyword === "type" && (m = e.error.match(/type "([^"]+)" is invalid\. Expected (.+)\.$/))) {
    return `Expected ${m[2].replace(/"/g, "").replace(/, /g, " or ")}, got ${m[1]}`;
  }
  if (e.keyword === "enum" && (m = e.error.match(/any of \[(.*)\]/))) {
    return `Must be one of ${m[1].replace(/",/g, "\", ")}`;
  }
  return e.error;
}

/** "Expected integer, got boolean" + "Expected string, got boolean" -> "Expected integer or string, got boolean". */
function mergeTypeMessages(messages: string[]): string {
  const expected: string[] = [];
  let got = "";
  const rest: string[] = [];
  for (const m of messages) {
    const r = m.match(/^Expected (.+), got (.+)$/);
    if (r) {
      expected.push(r[1]);
      got = r[2];
    } else rest.push(m);
  }
  if (expected.length) rest.unshift(`Expected ${expected.join(" or ")}, got ${got}`);
  return rest.join("\n");
}

/**
 * Flags map keys the schema does not declare. A map is closed when its schema
 * lists properties and neither allows additionalProperties nor preserves
 * unknown fields, which is how the API server itself treats typed objects.
 */
export function unknownFieldDiagnostics(doc: Document, root: SchemaNode, schemas: SchemaMap): Diagnostic[] {
  const out: Diagnostic[] = [];
  walk(doc.contents as ParsedNode | null, deref(root, schemas));
  return out;

  function walk(node: ParsedNode | null, schema: SchemaNode | undefined): void {
    if (!node || !schema) return;
    if (isMap(node)) {
      const props = schema.properties;
      const extra = typeof schema.additionalProperties === "object" ? schema.additionalProperties : undefined;
      const closed =
        !!props &&
        !extra &&
        schema.additionalProperties !== true &&
        schema["x-kubernetes-preserve-unknown-fields"] !== true;
      for (const pair of node.items as Pair<ParsedNode, ParsedNode | null>[]) {
        if (!isScalar(pair.key)) continue;
        const name = String(pair.key.value);
        const prop = props?.[name];
        if (prop) walk(pair.value, deref(prop, schemas));
        else if (extra) walk(pair.value, deref(extra, schemas));
        else if (closed) {
          const r = keyRange(pair);
          if (r) out.push({ from: r.from, to: r.to, severity: "error", source: SOURCE, message: `Unknown field "${name}"` });
        }
      }
    } else if (isSeq(node)) {
      const items = deref(schema.items, schemas);
      for (const item of node.items) walk(item as ParsedNode, items);
    }
  }
}
