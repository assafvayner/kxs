import { describe, expect, it } from "vitest";
import { completionOptions } from "./complete";
import type { CursorContext } from "./cursor";
import { toJsonSchema, type SchemaMap } from "./schema";

const schemas: SchemaMap = toJsonSchema({
  Root: {
    type: "object",
    properties: {
      apiVersion: { type: "string" },
      kind: { type: "string" },
      metadata: { description: "Standard metadata", allOf: [{ $ref: "#/components/schemas/ObjectMeta" }] },
      spec: { allOf: [{ $ref: "#/components/schemas/Spec" }] },
    },
  },
  ObjectMeta: { type: "object", properties: { name: { type: "string" }, labels: { type: "object", additionalProperties: { type: "string" } } } },
  Spec: {
    type: "object",
    required: ["template"],
    properties: {
      replicas: { type: "integer", description: "Desired pods" },
      paused: { type: "boolean" },
      template: { type: "object", properties: { spec: { type: "object" } } },
      containers: { type: "array", items: { allOf: [{ $ref: "#/components/schemas/Container" }] } },
    },
  },
  Container: { type: "object", properties: { image: { type: "string" }, imagePullPolicy: { type: "string", enum: ["Always", "IfNotPresent", "Never"] } } },
});
const root = schemas.Root;

function ctx(partial: Partial<CursorContext>): CursorContext {
  return { mode: "key", path: [], from: 0, word: "", siblings: [], indent: 0, ...partial };
}

describe("completionOptions", () => {
  it("offers undeclared sibling keys with type details and descriptions", () => {
    const opts = completionOptions(ctx({ path: ["spec"], siblings: ["replicas"], indent: 2 }), root, schemas);
    expect(opts.map((o) => o.label)).toEqual(["paused", "template", "containers"]);
    const template = opts.find((o) => o.label === "template")!;
    expect(template.detail).toBe("object");
    expect(template.boost).toBe(1);
    expect(opts.find((o) => o.label === "paused")!.detail).toBe("boolean");
    expect(opts.find((o) => o.label === "containers")!.detail).toBe("Container[]");
  });

  it("inserts scalars, objects, and arrays with the right continuation", () => {
    const opts = completionOptions(ctx({ path: ["spec"], indent: 2 }), root, schemas);
    expect(opts.find((o) => o.label === "replicas")!.apply).toBe("replicas: ");
    expect(opts.find((o) => o.label === "template")!.apply).toBe("template:\n    ");
    expect(opts.find((o) => o.label === "containers")!.apply).toBe("containers:\n    - ");
    expect(opts.find((o) => o.label === "replicas")!.info).toBe("Desired pods");
  });

  it("completes keys inside array items", () => {
    const opts = completionOptions(ctx({ path: ["spec", "containers", 0], siblings: ["image"], indent: 6 }), root, schemas);
    expect(opts.map((o) => o.label)).toEqual(["imagePullPolicy"]);
  });

  it("offers enum values and booleans in value position", () => {
    const e = completionOptions(ctx({ mode: "value", path: ["spec", "containers", 0], key: "imagePullPolicy" }), root, schemas);
    expect(e.map((o) => o.label)).toEqual(["Always", "IfNotPresent", "Never"]);
    const b = completionOptions(ctx({ mode: "value", path: ["spec"], key: "paused" }), root, schemas);
    expect(b.map((o) => o.label)).toEqual(["true", "false"]);
    expect(completionOptions(ctx({ mode: "value", path: ["spec"], key: "replicas" }), root, schemas)).toEqual([]);
  });

  it("returns nothing for unknown paths or open maps", () => {
    expect(completionOptions(ctx({ path: ["nope"] }), root, schemas)).toEqual([]);
    expect(completionOptions(ctx({ path: ["metadata", "labels"] }), root, schemas)).toEqual([]);
  });
});
