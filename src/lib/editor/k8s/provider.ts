import { Validator } from "@cfworker/json-schema";
import { api } from "../../api";
import { findKindSchema, toJsonSchema, type Gvk, type SchemaMap, type SchemaNode } from "./schema";

export interface ResolvedSchema {
  root: SchemaNode;
  schemas: SchemaMap;
  validator: Validator;
}

export interface SchemaProvider {
  /** Resolved schema for a kind, or null when the cluster does not serve it. */
  schemaFor(gvk: Gvk): Promise<ResolvedSchema | null>;
}

/** Fetches group/version documents once per tab and builds one validator per kind. */
export function createSchemaProvider(tabId: number): SchemaProvider {
  const docs = new Map<string, Promise<SchemaMap | null>>();
  const kinds = new Map<string, Promise<ResolvedSchema | null>>();

  function schemasFor(gvk: Gvk): Promise<SchemaMap | null> {
    const key = `${gvk.group}/${gvk.version}`;
    let p = docs.get(key);
    if (!p) {
      p = api.getOpenApiSchema(tabId, gvk.group, gvk.version).then((raw) => {
        if (raw === null) return null;
        const doc = JSON.parse(raw) as { components?: { schemas?: SchemaMap } };
        return toJsonSchema(doc.components?.schemas ?? {});
      });
      p.catch(() => docs.delete(key));
      docs.set(key, p);
    }
    return p;
  }

  return {
    schemaFor(gvk) {
      const key = `${gvk.group}/${gvk.version}/${gvk.kind}`;
      let p = kinds.get(key);
      if (!p) {
        p = schemasFor(gvk).then((schemas) => {
          const found = schemas && findKindSchema(schemas, gvk);
          if (!schemas || !found) return null;
          const validator = new Validator(
            { $ref: `#/components/schemas/${found.name}`, components: { schemas } },
            "7",
            false,
          );
          return { root: found.schema, schemas, validator };
        });
        p.catch(() => kinds.delete(key));
        kinds.set(key, p);
      }
      return p;
    },
  };
}

const providers = new Map<number, SchemaProvider>();

/** One provider per cluster tab so every editor opened in it shares the cached documents. */
export function schemaProviderFor(tabId: number): SchemaProvider {
  let p = providers.get(tabId);
  if (!p) {
    p = createSchemaProvider(tabId);
    providers.set(tabId, p);
  }
  return p;
}

export function dropSchemaProvider(tabId: number): void {
  providers.delete(tabId);
}
