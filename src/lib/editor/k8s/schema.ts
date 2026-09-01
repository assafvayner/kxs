export interface Gvk {
  group: string;
  version: string;
  kind: string;
}

export type SchemaNode = Record<string, unknown> & {
  type?: string | string[];
  properties?: Record<string, SchemaNode>;
  items?: SchemaNode;
  additionalProperties?: boolean | SchemaNode;
  required?: string[];
  enum?: unknown[];
  description?: string;
  $ref?: string;
  allOf?: SchemaNode[];
  oneOf?: SchemaNode[];
  anyOf?: SchemaNode[];
  not?: SchemaNode;
  format?: string;
  nullable?: boolean;
};

export type SchemaMap = Record<string, SchemaNode>;

const REF_PREFIX = "#/components/schemas/";
const KEEP_FORMATS = new Set(["date-time", "date", "time", "email", "ipv4", "ipv6", "uri", "uuid", "regex"]);
const KEEP_VENDOR_KEYS = new Set(["x-kubernetes-group-version-kind", "x-kubernetes-preserve-unknown-fields"]);

export function parseApiVersion(apiVersion: string): { group: string; version: string } {
  const i = apiVersion.indexOf("/");
  return i === -1
    ? { group: "", version: apiVersion }
    : { group: apiVersion.slice(0, i), version: apiVersion.slice(i + 1) };
}

export function sameGvk(a: Gvk, b: Gvk): boolean {
  return a.group === b.group && a.version === b.version && a.kind === b.kind;
}

/** Rewrites Kubernetes OpenAPI 3.0 component schemas into draft-07 JSON Schema, in place. */
export function toJsonSchema(schemas: SchemaMap): SchemaMap {
  for (const s of Object.values(schemas)) rewrite(s);
  return schemas;
}

function rewrite(s: SchemaNode): void {
  if (s["x-kubernetes-int-or-string"] === true && !s.oneOf) s.type = ["integer", "string"];
  if (s.nullable === true) {
    if (typeof s.type === "string") s.type = [s.type, "null"];
    else if (Array.isArray(s.type) && !s.type.includes("null")) s.type = [...s.type, "null"];
  }
  delete s.nullable;
  if (typeof s.format === "string" && !KEEP_FORMATS.has(s.format)) delete s.format;
  for (const k of Object.keys(s)) {
    if (k.startsWith("x-kubernetes-") && !KEEP_VENDOR_KEYS.has(k)) delete s[k];
  }
  if (s.properties) for (const p of Object.values(s.properties)) rewrite(p);
  if (s.items) rewrite(s.items);
  if (typeof s.additionalProperties === "object") rewrite(s.additionalProperties);
  if (s.not) rewrite(s.not);
  s.allOf?.forEach(rewrite);
  s.oneOf?.forEach(rewrite);
  s.anyOf?.forEach(rewrite);
}

export function findKindSchema(schemas: SchemaMap, gvk: Gvk): { name: string; schema: SchemaNode } | null {
  for (const [name, schema] of Object.entries(schemas)) {
    const gvks = schema["x-kubernetes-group-version-kind"] as Gvk[] | undefined;
    if (gvks?.some((g) => sameGvk(g, gvk))) return { name, schema };
  }
  return null;
}

export function refName(node: SchemaNode | undefined): string | undefined {
  const ref = node?.$ref ?? (node?.allOf?.length === 1 ? node.allOf[0].$ref : undefined);
  return ref?.startsWith(REF_PREFIX) ? ref.slice(REF_PREFIX.length) : undefined;
}

/** Follows $ref and single-$ref allOf wrappers, keeping the wrapper's description when it has one. */
export function deref(node: SchemaNode | undefined, schemas: SchemaMap, depth = 0): SchemaNode | undefined {
  if (!node || depth > 8) return node;
  const name = refName(node);
  if (!name) return node;
  const inner = deref(schemas[name], schemas, depth + 1);
  if (!inner) return undefined;
  return node.description ? { ...inner, description: node.description } : inner;
}

/** Schema for the value reached by following keys (map fields) and numbers (array items). */
export function schemaAt(root: SchemaNode, schemas: SchemaMap, path: Array<string | number>): SchemaNode | undefined {
  let node = deref(root, schemas);
  for (const seg of path) {
    if (!node) return undefined;
    if (typeof seg === "number") {
      node = deref(node.items, schemas);
    } else {
      const prop = node.properties?.[seg];
      const extra = typeof node.additionalProperties === "object" ? node.additionalProperties : undefined;
      node = deref(prop ?? extra, schemas);
    }
  }
  return node;
}

/** Short type name for completion details: the referenced type's last path segment, or the primitive type. */
export function typeLabel(node: SchemaNode | undefined, schemas: SchemaMap): string {
  if (!node) return "";
  const name = refName(node);
  if (name) {
    const target = deref(node, schemas);
    if (target?.oneOf && !target.properties) return typeLabel(target, schemas);
    return name.slice(name.lastIndexOf(".") + 1);
  }
  if (node.enum) return "enum";
  if (Array.isArray(node.type)) return node.type.join(" | ");
  if (node.type === "array") return `${typeLabel(node.items, schemas) || "any"}[]`;
  if (node.oneOf) return node.oneOf.map((o) => typeLabel(o, schemas)).join(" | ");
  return node.type ?? "object";
}
