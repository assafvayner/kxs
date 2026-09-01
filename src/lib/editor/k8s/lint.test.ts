import { describe, expect, it } from "vitest";
import { Validator } from "@cfworker/json-schema";
import { gvkFromText, lintDocument } from "./lint";
import type { SchemaProvider } from "./provider";
import { toJsonSchema, type SchemaMap } from "./schema";

const schemas: SchemaMap = toJsonSchema({
  Pod: {
    type: "object",
    properties: { apiVersion: { type: "string" }, kind: { type: "string" }, spec: { type: "object", properties: { hostNetwork: { type: "boolean" } } } },
    "x-kubernetes-group-version-kind": [{ group: "", version: "v1", kind: "Pod" }],
  },
});
const provider: SchemaProvider = {
  async schemaFor(gvk) {
    if (gvk.kind !== "Pod") return null;
    return { root: schemas.Pod, schemas, validator: new Validator({ $ref: "#/components/schemas/Pod", components: { schemas } }, "7", false) };
  },
};
const pod = { group: "", version: "v1", kind: "Pod" };

describe("gvkFromText", () => {
  it("reads apiVersion and kind from the head of the document", () => {
    expect(gvkFromText("apiVersion: apps/v1\nkind: Deployment\n")).toEqual({ group: "apps", version: "v1", kind: "Deployment" });
    expect(gvkFromText("apiVersion: v1 # core\nkind: Pod")).toEqual(pod);
  });
  it("returns null when either line is missing", () => {
    expect(gvkFromText("kind: Pod\n")).toBeNull();
  });
});

describe("lintDocument", () => {
  it("reports schema errors and unknown fields", async () => {
    const text = "apiVersion: v1\nkind: Pod\nspec:\n  hostNetwork: yes please\n  hostNet: true\n";
    const diags = await lintDocument(text, pod, provider);
    expect(diags.map((d) => d.message)).toEqual(["Expected boolean, got string", 'Unknown field "hostNet"']);
  });

  it("reports only parse errors when the YAML is broken", async () => {
    const diags = await lintDocument("apiVersion: v1\nkind: Pod\nspec: [\n", pod, provider);
    expect(diags).toHaveLength(1);
    expect(diags[0].source).toBe("yaml");
    expect(diags[0].severity).toBe("error");
  });

  it("adds an info diagnostic on kind when the cluster has no schema", async () => {
    const diags = await lintDocument("apiVersion: v1\nkind: Widget\n", { group: "", version: "v1", kind: "Widget" }, provider);
    expect(diags).toHaveLength(1);
    expect(diags[0]).toMatchObject({ severity: "info", from: "apiVersion: v1\n".length, to: "apiVersion: v1\n".length + 4 });
    expect(diags[0].message).toContain("v1 Widget");
  });

  it("stays quiet when the provider fails", async () => {
    const failing: SchemaProvider = { schemaFor: () => Promise.reject(new Error("offline")) };
    expect(await lintDocument("apiVersion: v1\nkind: Pod\n", pod, failing)).toEqual([]);
  });

  it("returns nothing for an empty document", async () => {
    expect(await lintDocument("", pod, provider)).toEqual([]);
  });
});
