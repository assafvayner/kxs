import { describe, expect, it } from "vitest";
import { Validator } from "@cfworker/json-schema";
import { parseDocument } from "yaml";
import { schemaDiagnostics, unknownFieldDiagnostics } from "./diagnostics";
import { pointerSegments, resolvePointer } from "./positions";
import { toJsonSchema, type SchemaMap } from "./schema";

const schemas: SchemaMap = toJsonSchema({
  Root: {
    type: "object",
    properties: {
      apiVersion: { type: "string" },
      kind: { type: "string" },
      metadata: { allOf: [{ $ref: "#/components/schemas/ObjectMeta" }] },
      spec: { allOf: [{ $ref: "#/components/schemas/Spec" }] },
    },
    "x-kubernetes-group-version-kind": [{ group: "apps", version: "v1", kind: "Deployment" }],
  },
  ObjectMeta: {
    type: "object",
    properties: { name: { type: "string" }, labels: { type: "object", additionalProperties: { type: "string" } } },
  },
  Spec: {
    type: "object",
    required: ["selector"],
    properties: {
      replicas: { type: "integer" },
      selector: { type: "object" },
      maxSurge: { allOf: [{ $ref: "#/components/schemas/IntOrString" }] },
      policy: { type: "string", enum: ["Always", "Never"] },
      containers: { type: "array", items: { allOf: [{ $ref: "#/components/schemas/Container" }] } },
      raw: { type: "object", "x-kubernetes-preserve-unknown-fields": true },
    },
  },
  Container: { type: "object", properties: { image: { type: "string" } } },
  IntOrString: { oneOf: [{ type: "integer" }, { type: "string" }] },
});
const validator = new Validator({ $ref: "#/components/schemas/Root", components: { schemas } }, "7", false);

const text = [
  "apiVersion: apps/v1",
  "kind: Deployment",
  "metadata:",
  "  name: web",
  "  labels:",
  "    app.kubernetes.io/name: 3",
  "spec:",
  "  replicas: three",
  "  replcas: 1",
  "  maxSurge: true",
  "  policy: Sometimes",
  "  containers:",
  "    - image: nginx",
  "      imag: x",
  "  raw:",
  "    anything: goes",
  "",
].join("\n");

function at(needle: string, from = 0): [number, number] {
  const i = text.indexOf(needle, from);
  return [i, i + needle.length];
}

describe("pointerSegments", () => {
  it("splits and unescapes JSON pointers", () => {
    expect(pointerSegments("#/metadata/labels/app.kubernetes.io~1name")).toEqual(["metadata", "labels", "app.kubernetes.io/name"]);
    expect(pointerSegments("#")).toEqual([]);
  });
});

describe("resolvePointer", () => {
  it("returns the value node and its owning pair", () => {
    const doc = parseDocument(text);
    const r = resolvePointer(doc, "#/spec/replicas");
    expect(r.node?.range?.slice(0, 2)).toEqual(at("three"));
    expect(r.pair?.key.range?.slice(0, 2)).toEqual(at("replicas"));
  });
  it("reports a missing key with the parent map", () => {
    const doc = parseDocument(text);
    const r = resolvePointer(doc, "#/spec/selector");
    expect(r.node).toBeNull();
    expect(r.pair?.key.range?.slice(0, 2)).toEqual([text.indexOf("spec:"), text.indexOf("spec:") + 4]);
  });
});

describe("schemaDiagnostics", () => {
  const doc = parseDocument(text);
  const diags = schemaDiagnostics(doc, validator.validate(doc.toJS()).errors);
  const find = (needle: string) => diags.find((d) => d.from === at(needle)[0]);

  it("puts type errors on the value", () => {
    const d = find("three")!;
    expect([d.from, d.to]).toEqual(at("three"));
    expect(d.message).toBe("Expected integer, got string");
  });

  it("merges oneOf branch errors into one message", () => {
    const d = find("true")!;
    expect(d.message).toBe("Expected integer or string, got boolean");
    expect(diags.filter((x) => x.from === d.from)).toHaveLength(1);
  });

  it("puts required errors on the owning key", () => {
    const d = diags.find((x) => x.message.includes("selector"))!;
    expect([d.from, d.to]).toEqual([text.indexOf("spec:"), text.indexOf("spec:") + 4]);
    expect(d.message).toBe('Missing required field "selector"');
  });

  it("explains enum errors", () => {
    expect(find("Sometimes")!.message).toBe('Must be one of "Always", "Never"');
  });

  it("unescapes pointers for map keys with slashes", () => {
    const d = find("3")!;
    expect(d.message).toBe("Expected string, got number");
  });

  it("emits nothing for unknown fields (the walker owns those)", () => {
    expect(diags.some((d) => d.message.includes("replcas"))).toBe(false);
    expect(diags.every((d) => d.severity === "error" && d.source === "k8s-schema")).toBe(true);
  });
});

describe("unknownFieldDiagnostics", () => {
  const doc = parseDocument(text);
  const diags = unknownFieldDiagnostics(doc, schemas.Root, schemas);

  it("flags undeclared keys at the key range, through refs and arrays", () => {
    expect(diags.map((d) => [d.from, d.to, d.message])).toEqual([
      [...at("replcas"), 'Unknown field "replcas"'],
      [...at("imag", text.indexOf("imag: x")), 'Unknown field "imag"'],
    ]);
  });

  it("leaves additionalProperties maps and preserve-unknown-fields objects open", () => {
    expect(diags.some((d) => d.message.includes("app.kubernetes.io"))).toBe(false);
    expect(diags.some((d) => d.message.includes("anything"))).toBe(false);
  });
});

describe("schemaDiagnostics inside sequences and with union types", () => {
  const s: SchemaMap = toJsonSchema({
    Root: {
      type: "object",
      properties: {
        items: { type: "array", items: { type: "object", required: ["name"], properties: { name: { type: "string" } } } },
        count: { type: "integer", nullable: true },
        size: { type: "object", "x-kubernetes-int-or-string": true },
      },
    },
  });
  const v = new Validator({ $ref: "#/components/schemas/Root", components: { schemas: s } }, "7", false);
  const src = "items:\n  - other: 1\ncount: yes\nsize: true\n";
  const doc = parseDocument(src);
  const diags = schemaDiagnostics(doc, v.validate(doc.toJS()).errors);

  it("puts a required error inside an array item on the key owning the array", () => {
    const d = diags.find((x) => x.message.includes("name"))!;
    expect([d.from, d.to]).toEqual([0, 5]);
  });

  it("lists every alternative of a union type", () => {
    expect(diags.find((x) => x.from === src.indexOf("yes"))!.message).toBe("Expected integer or null, got string");
    expect(diags.find((x) => x.from === src.indexOf("true"))!.message).toBe("Expected integer or string, got boolean");
  });
});
