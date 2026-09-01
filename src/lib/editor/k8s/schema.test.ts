import { describe, expect, it } from "vitest";
import {
  deref,
  findKindSchema,
  parseApiVersion,
  schemaAt,
  toJsonSchema,
  typeLabel,
  type SchemaMap,
} from "./schema";

function fixture(): SchemaMap {
  return {
    "io.k8s.api.apps.v1.Deployment": {
      type: "object",
      properties: {
        apiVersion: { type: "string" },
        kind: { type: "string" },
        metadata: { description: "Standard metadata", default: {}, allOf: [{ $ref: "#/components/schemas/io.k8s.meta.v1.ObjectMeta" }] },
        spec: { allOf: [{ $ref: "#/components/schemas/io.k8s.api.apps.v1.DeploymentSpec" }] },
      },
      "x-kubernetes-group-version-kind": [{ group: "apps", version: "v1", kind: "Deployment" }],
    },
    "io.k8s.meta.v1.ObjectMeta": {
      type: "object",
      properties: {
        name: { type: "string" },
        labels: { type: "object", additionalProperties: { type: "string" } },
        creationTimestamp: { type: "string", format: "date-time", nullable: true },
      },
    },
    "io.k8s.api.apps.v1.DeploymentSpec": {
      type: "object",
      required: ["template"],
      properties: {
        replicas: { type: "integer", format: "int32", "x-kubernetes-patch-strategy": "merge" },
        maxSurge: { allOf: [{ $ref: "#/components/schemas/IntOrString" }] },
        containers: { type: "array", items: { allOf: [{ $ref: "#/components/schemas/Container" }] } },
        raw: { type: "object", "x-kubernetes-preserve-unknown-fields": true },
      },
    },
    Container: { type: "object", properties: { image: { type: "string" }, policy: { type: "string", enum: ["Always", "Never"] } } },
    IntOrString: { format: "int-or-string", oneOf: [{ type: "integer" }, { type: "string" }] },
    Other: { type: "object", "x-kubernetes-int-or-string": true },
  };
}

describe("parseApiVersion", () => {
  it("splits group and version", () => {
    expect(parseApiVersion("apps/v1")).toEqual({ group: "apps", version: "v1" });
  });
  it("treats a bare version as the core group", () => {
    expect(parseApiVersion("v1")).toEqual({ group: "", version: "v1" });
  });
});

describe("toJsonSchema", () => {
  const s = toJsonSchema(fixture());

  it("drops non-standard formats and keeps date-time", () => {
    expect(s["io.k8s.api.apps.v1.DeploymentSpec"].properties!.replicas.format).toBeUndefined();
    expect(s["io.k8s.meta.v1.ObjectMeta"].properties!.creationTimestamp.format).toBe("date-time");
    expect(s.IntOrString.format).toBeUndefined();
  });

  it("turns nullable into a null-accepting type", () => {
    const ts = s["io.k8s.meta.v1.ObjectMeta"].properties!.creationTimestamp;
    expect(ts.type).toEqual(["string", "null"]);
    expect(ts.nullable).toBeUndefined();
  });

  it("types int-or-string when the source lacks oneOf", () => {
    expect(s.Other.type).toEqual(["integer", "string"]);
  });

  it("strips vendor keys except the ones the walker needs", () => {
    expect(s["io.k8s.api.apps.v1.DeploymentSpec"].properties!.replicas["x-kubernetes-patch-strategy"]).toBeUndefined();
    expect(s["io.k8s.api.apps.v1.Deployment"]["x-kubernetes-group-version-kind"]).toBeDefined();
    expect(s["io.k8s.api.apps.v1.DeploymentSpec"].properties!.raw["x-kubernetes-preserve-unknown-fields"]).toBe(true);
  });

  it("never adds additionalProperties", () => {
    expect(s["io.k8s.meta.v1.ObjectMeta"].additionalProperties).toBeUndefined();
  });
});

describe("findKindSchema", () => {
  it("finds a kind by group/version/kind", () => {
    expect(findKindSchema(fixture(), { group: "apps", version: "v1", kind: "Deployment" })?.name).toBe(
      "io.k8s.api.apps.v1.Deployment",
    );
  });
  it("returns null for an unknown kind", () => {
    expect(findKindSchema(fixture(), { group: "apps", version: "v1", kind: "Nope" })).toBeNull();
  });
});

describe("deref and schemaAt", () => {
  const s = toJsonSchema(fixture());
  const root = s["io.k8s.api.apps.v1.Deployment"];

  it("follows allOf-wrapped refs and keeps the wrapper description", () => {
    const meta = deref(root.properties!.metadata, s)!;
    expect(meta.properties!.name.type).toBe("string");
    expect(meta.description).toBe("Standard metadata");
  });

  it("walks keys and array indexes", () => {
    expect(schemaAt(root, s, ["spec", "containers", 0, "image"])?.type).toBe("string");
    expect(schemaAt(root, s, ["spec", "containers", 0])?.properties!.policy.enum).toEqual(["Always", "Never"]);
  });

  it("walks into additionalProperties maps", () => {
    expect(schemaAt(root, s, ["metadata", "labels", "app"])?.type).toBe("string");
  });

  it("returns undefined for unknown paths", () => {
    expect(schemaAt(root, s, ["spec", "nope"])).toBeUndefined();
  });

  it("labels types for completion details", () => {
    const spec = deref(root.properties!.spec, s)!;
    expect(typeLabel(spec.properties!.replicas, s)).toBe("integer");
    expect(typeLabel(spec.properties!.maxSurge, s)).toBe("integer | string");
    expect(typeLabel(spec.properties!.containers, s)).toBe("Container[]");
    expect(typeLabel(root.properties!.metadata, s)).toBe("ObjectMeta");
    expect(typeLabel(deref(spec.properties!.containers, s)!.items!, s)).toBe("Container");
  });
});
