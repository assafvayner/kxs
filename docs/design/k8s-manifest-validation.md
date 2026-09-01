# Kubernetes manifest validation in the YAML editor

Status: phases 1 and 3 implemented (schema validation, unknown-field
detection, key and enum completion) in `src/lib/editor/k8s/`; phase 2
(dry-run diagnostics) is not implemented.

## Goals

- Show schema errors inline, as the user types, before they press Validate:
  unknown fields, wrong types, missing required fields, bad enum values.
- Work for every kind the cluster serves, including CRDs, because the schema
  comes from the connected API server rather than a bundled snapshot.
- Keep the existing Validate (server dry-run) and Apply flow, and surface
  dry-run failures at the offending line instead of only in the status bar.
- Stay small: no language server, no worker, no bundled schema corpus.

## Non-goals

- Admission-webhook or policy validation (OPA, Kyverno). Only the server can
  answer those; dry-run already covers them.
- Semantic cross-field checks beyond what the OpenAPI schema encodes.
- Validation in the read-only YAML view. It shows server state, which is valid
  by construction.
- Multi-document files. `apply_edit` treats the buffer as one object and the
  editor is opened on one resource; the linter validates the first document
  and flags extra documents as a warning.

## Architecture

```
 EditorView ─ doc changes ─▶ linter(k8sLintSource)  ── Diagnostic[] ─▶ lintGutter + underline
                                   │
                                   ├─ parseDocument (yaml)  ─▶ AST with node.range
                                   ├─ gvkField (StateField) ─▶ {group, version, kind} from apiVersion/kind
                                   └─ SchemaProvider.get(gvk) ─ Tauri IPC ─▶ get_openapi_schema
                                                                                  │
                                                          kube Client ── GET /openapi/v3/apis/<g>/<v>
                                                          (cached per session by (g/v, hash))
```

Validation runs in the webview with `@cfworker/json-schema` against a JSON
Schema derived from the API server's OpenAPI v3 document for the resource's
group/version. Rust fetches and caches the spec; the frontend extracts the one
schema it needs, rewrites it to JSON Schema, and validates the parsed YAML.
Diagnostics are positioned by walking the `yaml` AST with the validator's
`instanceLocation` pointer.

### Why this over the alternatives

| Option | Bundle | Offline | CRDs | Positions | Verdict |
| --- | --- | --- | --- | --- | --- |
| A. Client-side JSON Schema (chosen) | `yaml` ~40 KB + `@cfworker/json-schema` ~12 KB min+gz | yes once the g/v spec is cached | yes, server publishes CRD schemas under `/openapi/v3/apis/<group>/<version>` | exact, via AST ranges | best fit |
| B. Server dry-run only | 0 | no | yes | approximate, parsed out of the Status message | keep as a second source, not the primary |
| C. yaml-language-server in a worker | ~1.5 MB + worker plumbing | yes | needs schema URLs per kind | exact | too heavy for what it adds |
| Bundled yannh/kubernetes-json-schema | ~50 MB for one version, or per-kind lazy files | yes | no | exact | wrong shape: kxs is always connected to the cluster it edits |

`@cfworker/json-schema` is chosen over `ajv` because it does not compile
schemas with `new Function`, so it stays fast to start and has no dependence on
the webview's eval policy. It supports draft-07 and 2019-09, which covers the
Kubernetes OpenAPI subset after the rewrite described below.

Option B remains valuable because the server checks things the schema cannot
(immutable fields, quota, admission). It becomes a second diagnostic source
that runs only on explicit Validate or Apply, described in "Dry-run as
diagnostics".

## Schema source: OpenAPI v3 from the cluster

The API server exposes `GET /openapi/v3` listing every group/version with an
immutable `serverRelativeURL` that carries a content hash. `GET
/openapi/v3/apis/apps/v1` (or `/openapi/v3/api/v1` for core) returns an
OpenAPI 3.0 document. Its `components.schemas` map holds one entry per type,
keyed like `io.k8s.api.apps.v1.Deployment`, and each top-level resource schema
carries `x-kubernetes-group-version-kind: [{group, version, kind}]`, which is
how the frontend finds the entry for a GVK without guessing the key format.
CRD schemas appear the same way, keyed by the CRD's own package-style name
(for example `com.example.v1.Widget`), with `x-kubernetes-group-version-kind`
set.

References inside the document are local (`#/components/schemas/...`), so the
document is self-contained per group/version. The core `v1` document is about
1.5 MB uncompressed and `apps/v1` about 700 KB; CRD documents are typically
tens of KB.

### Rust IPC surface

Add to `kxs-cluster` a `schema.rs` module and two Tauri commands in
`cluster_ipc.rs`, following the existing `session_of(&sessions, tab_id)` pattern.

```rust
// kxs-cluster/src/schema.rs
pub struct OpenApiCache {
    // key: "apis/apps/v1" or "api/v1"; value: (hash from serverRelativeURL, raw JSON)
    docs: tokio::sync::Mutex<HashMap<String, (String, Arc<str>)>>,
}

pub async fn openapi_index(client: &Client) -> Result<HashMap<String, String>, String>;
// GET /openapi/v3, returns path -> hash

pub async fn openapi_document(
    client: &Client,
    cache: &OpenApiCache,
    group: &str,
    version: &str,
) -> Result<Arc<str>, String>;
// resolves the path for (group, version), consults the index for the current hash,
// returns the cached document if the hash matches, otherwise GET the serverRelativeURL
```

Both use `client.request_text(http::Request<Vec<u8>>)` from kube 1.x, which
already exists on the `Client` held in `ClusterSession`. The cache lives on
`ClusterSession` alongside `client` so it is dropped with the tab. Because the
server URL is content-addressed, a hash comparison against the index is the
only invalidation needed; the index call itself is small (one JSON object of
paths) and can be re-fetched lazily at most once per minute per session.

Tauri command:

```rust
#[tauri::command]
pub async fn get_openapi_schema(
    tab_id: u32,
    group: String,
    version: String,
    sessions: State<'_, Sessions>,
) -> Result<String, String>; // raw OpenAPI v3 JSON for that group/version
```

Frontend `api.ts` gains `getOpenApiSchema(tabId, group, version): Promise<string>`.
Returning the whole group/version document, rather than one kind, keeps the
IPC surface dumb and lets the frontend cache one parsed document for every kind
in that group. Shipping the raw string over IPC avoids a serde round-trip of a
multi-megabyte value in Rust.

The `group`/`version` for the initial resource is already known: `YamlEditView`
receives `resourceKind: ResourceKind` with `group`, `version`, `kind`. The
StateField below re-derives the GVK from the buffer so that a user who edits
`apiVersion` gets the matching schema, and falls back to `resourceKind` when the
buffer's header is unparseable.

### OpenAPI 3.0 schema to JSON Schema

The Kubernetes OpenAPI schemas are close to draft-07 but need a rewrite pass
before `@cfworker/json-schema` will accept them. The pass is a pure function
over the `components.schemas` object, applied once per document and cached:

- Keep `$ref`s as-is; register the whole `components.schemas` map with the
  validator so `#/components/schemas/X` resolves.
- `nullable: true` becomes `type: [T, "null"]`.
- `x-kubernetes-int-or-string: true` becomes `type: ["integer", "string"]`;
  the source schema usually already carries `oneOf` for this, so only add when
  absent.
- Unknown fields are not handled through `additionalProperties: false`.
  `@cfworker/json-schema` also reports every declared-but-failing property as
  an additional property, which doubles every error. Instead
  `unknownFieldDiagnostics` walks the YAML AST alongside the schema and flags
  keys missing from `properties` when the object has properties, no
  `additionalProperties`, and no `x-kubernetes-preserve-unknown-fields`. That
  is what turns a typo like `replcas` into an error, with the squiggle on the
  key itself.
- `format` values other than the standard set (`int32`, `int64`, `date-time`,
  `byte`) are dropped so the validator does not reject them.
- Strip the `x-kubernetes-*` vendor keys after they have been consumed.
- Leave `metadata.managedFields`, `metadata.resourceVersion`, and `status`
  alone. Server YAML always contains them, `strip_server_fields` removes them
  from the patch, and the server schema already describes them.

### Frontend extension

New file `src/lib/editor/k8sLint.ts` exporting one entry point:

```ts
export interface SchemaProvider {
  /** Resolved JSON Schema for a GVK, or null when the cluster has no such type. */
  schemaFor(gvk: Gvk): Promise<ResolvedSchema | null>;
}

export function k8sLint(provider: SchemaProvider, fallback: Gvk): Extension {
  return [
    gvkField.init(() => fallback),
    linter(k8sLintSource(provider), {
      delay: 400,
      needsRefresh: (u) => u.startState.field(gvkField) !== u.state.field(gvkField),
    }),
    lintGutter(),
  ];
}
```

`gvkField` is a `StateField<Gvk>` whose `update` cheaply rescans the first
document for `apiVersion:` and `kind:` lines when `tr.docChanged` (a regex over
the first 4 KB is enough; the full parse happens in the lint source). It falls
back to the previous value when the lines are missing so the schema does not
flap mid-edit.

`k8sLintSource` is an async `LintSource`:

1. `parseDocument(view.state.doc.toString(), { lineCounter, keepSourceTokens: false })`
   from `yaml`. Parse errors and warnings from `doc.errors` / `doc.warnings`
   become diagnostics directly using `err.pos` (already document offsets).
   If there are parse errors, stop here; schema validation of a broken tree
   produces noise.
2. `const schema = await provider.schemaFor(state.field(gvkField))`. On `null`
   return one `info` diagnostic on the `kind:` line: "no schema for
   apps/v1 Foo on this cluster". On IPC failure return no diagnostics and log;
   an offline cluster must not make the editor red.
3. `validator.validate(doc.toJS())` with the resolved schema. Every returned
   error `{instanceLocation, keyword, keywordLocation, error}` is mapped to a
   range (next section) and pushed as `{from, to, severity: "error", message,
   source: "k8s-schema"}`.
4. Deduplicate by `(from, to, message)`; `oneOf`/`anyOf` branches produce
   repeated errors for the same node.

The provider implementation in `src/lib/editor/k8sSchemaProvider.ts` wraps the
IPC: a per-tab `Map<"g/v", Promise<ParsedDocument>>` so concurrent lint runs
share one fetch, plus a per-GVK cache of the rewritten schema. `@cfworker`
`Validator` instances are constructed once per GVK and reused.

### Position mapping

`@cfworker/json-schema` reports `instanceLocation` as a JSON pointer,
`#/spec/template/spec/containers/0/image`. The `yaml` AST exposes
`node.range: [start, valueEnd, nodeEnd]` in document offsets, which are also
CodeMirror offsets because both index the same string.

```
resolve(doc, pointer):
  node = doc.contents
  parentPair = null
  for seg in pointer segments (unescape ~1 ~0):
    if isMap(node):
      pair = node.items.find(p => String(p.key.value) === seg)
      if !pair: return { node, pair: parentPair, missing: seg }
      parentPair = pair; node = pair.value
    elif isSeq(node):
      node = node.items[Number(seg)]
    else: break
  return { node, pair: parentPair }
```

Range selection by error kind:

- `type`, `enum`, `pattern`, `minimum`, `format`, `const`: the value node,
  `[node.range[0], node.range[1]]`.
- `required`: the pointer names the object that is missing a key. Highlight
  the parent pair's key (`pair.key.range`) so the squiggle sits on
  `containers:` rather than the whole nested block. At the document root,
  highlight the `kind:` line.
- `additionalProperties`: the pointer names the object; the offending
  property name is in the message ("Property "replcas" does not match
  additional properties schema"). Extract it, find that pair, and highlight
  its key. If extraction fails, fall back to the object's first line.
- Only leaf keywords are mapped (`type`, `enum`, `required`, `pattern`,
  bounds, lengths, `format`, `const`); `$ref`, `properties`, `allOf`,
  `oneOf`, `anyOf`, and `additionalProperties` wrapper errors are dropped.
  Leaf errors that land on one range merge into a single diagnostic, so a
  boolean where `IntOrString` is expected reads "Expected integer or string,
  got boolean" rather than two messages.
- While the document has a YAML parse error (typically mid-keystroke, before
  the colon of a new key), only the parse error is shown; schema diagnostics
  return on the next parseable state.

When the node has no range (an implicit null value like `foo:` with nothing
after it), use `pair.key.range[1]` as a zero-width position; CodeMirror renders
a zero-width diagnostic as a small marker rather than nothing.

Mapping `yaml` positions to CodeMirror is direct: both are UTF-16 code unit
offsets into the same string. `lineCounter` is only needed for messages.

### Dry-run as diagnostics

The Validate button and vim `:w` call `api.applyYaml(..., dryRun=true)`. Today
`apply_edit` returns `Err(e.to_string())` and the view prints the text under the
editor. The server's error message has a stable shape:

```
Deployment.apps "web" is invalid: [spec.replicas: Invalid value: -1: must be greater than or equal to 0, spec.template.spec.containers[0].image: Required value]
```

Change `apply_edit`'s dry-run error path to return a structured value
alongside the message:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyFailure {
    pub message: String,               // full server text, shown as today
    pub causes: Vec<FieldCause>,       // parsed field paths
}
pub struct FieldCause { pub field: String, pub message: String }
```

`kube::Error::Api(ErrorResponse)` does not carry `Status.details.causes`, so
parse them out of `message` with a regex over the bracketed list:
`([A-Za-z0-9_.\[\]]+): ((?:Invalid|Required|Unsupported|Forbidden|Duplicate)[^,\]]*)`.
Field paths use dotted notation with `[i]` indexes; convert to a JSON pointer
(`spec.containers[0].image` becomes `#/spec/containers/0/image`) and reuse the
same `resolve` walker. The YAML editor dispatches
`setDiagnostics(state, [...schemaDiagnostics, ...dryRunDiagnostics])` with
`source: "k8s-server"` and `severity: "error"`. Because `setDiagnostics`
replaces the whole set, the lint source's next run must merge the server
diagnostics back in; keep them in a `StateField<Diagnostic[]>` that the
linter appends and that clears on the next successful dry-run or on
`docChanged` when the changed range overlaps a server diagnostic.

Immutable-field conflicts from `apply_edit` ("apiVersion/kind/name/namespace
cannot be changed in the editor") map to the `metadata.name` or `kind` line.
The staleness conflict ("changed on the server since you opened the editor")
lists dotted paths and maps the same way with `severity: "warning"`.

The status bar keeps its current text. The `apply-err` pane can shrink to the
first line once diagnostics carry the detail.

### Composition in `src/lib/editor/setup.ts`

`YamlEditView` composes no extensions itself; it renders `<CodeEditor>`, and
vim lives in a compartment inside that component. Linting joins the list that
`buildExtensions` returns, behind a new option, and `CodeEditor` gains a
`lint` prop that `YamlEditView` fills from `resourceKind`:

```ts
buildExtensions({ readOnly, onChange, lint })
// ...
lint ? [k8sLint(lint.provider, lint.gvk), serverDiagnosticsField, lintGutter()] : [],
```

A "Problems: N" counter in the detail bar comes free from
`diagnosticCount(state)`; `nextDiagnostic` can be bound to `]d` in vim mode
and F8 otherwise.

## Phasing

Phase 1, schema from the cluster for the resource being edited:
`schema.rs` + `get_openapi_schema` IPC, the OpenAPI rewrite, `k8sLint`,
position mapping for the four common error kinds, unit tests for the rewrite
and the walker in vitest (pure functions over fixture JSON and YAML strings),
one Rust test against the kind-local cluster gated `#[ignore]` like
`connects_to_kind_local`. Bundled schemas are deliberately not part of this
phase: kxs is never editing a resource without a live session, so "phase 1
built-in schemas" would be throwaway work.

Phase 2, dry-run diagnostics: `ApplyFailure` structured error, the regex
cause parser with tests on captured server messages, the merge field, and the
Problems counter.

Phase 3, completion and hover: a `CompletionSource` that walks the same AST to
the cursor's enclosing map, resolves its schema node, and offers
`properties` keys not yet present (with `description` as `info`) and `enum`
values in value position. Hover uses `hoverTooltip` with the schema node's
`description`. Both reuse the resolved schema and the walker, so the new code
is mostly cursor-to-pointer.

## Size and latency budget

| Item | Estimate |
| --- | --- |
| `yaml` | ~40 KB min+gz, already a plausible dependency for the CodeMirror `lang-yaml` work |
| `@cfworker/json-schema` | ~12 KB min+gz |
| `@codemirror/lint` | ~8 KB min+gz, shares `@codemirror/view` with the editor |
| First lint of a Deployment | one `/openapi/v3` index GET (~30 KB) plus one `apps/v1` GET (~700 KB, gzip on the wire), typically 100 to 400 ms on a remote cluster, then cached for the tab |
| Rewrite of `apps/v1` schemas | ~5 ms, once per document |
| Validate a 300-line Deployment | under 5 ms; the linter's 400 ms debounce dominates |
| Memory per tab | one parsed OpenAPI document per touched group/version, ~5 MB for core `v1`, released with the session |

## Open questions

- Should `additionalProperties: false` be applied to `metadata` and its
  `annotations`/`labels`? Those are `additionalProperties: {type: string}`
  in the source schema so they stay open; only typed objects close. Confirm
  against a few real CRDs whose `spec` uses `x-kubernetes-preserve-unknown-fields`.
- The core `v1` document is large. If first-lint latency on slow clusters is
  noticeable, fetch it eagerly when a session connects rather than on first
  edit, since Pods, ConfigMaps, and Secrets are the most-edited kinds.
- Whether to store the OpenAPI cache on disk keyed by hash so a reopened tab
  is warm. Content-addressed URLs make this safe, but the app has no disk
  cache today.
- Aggregated APIs (metrics.k8s.io) can be missing from `/openapi/v3` when the
  aggregated server is down; the provider must treat a 404 or 503 for one
  group/version as "no schema" rather than an error.
- Line ending normalization: `getResourceYaml` returns `\n`, but a paste can
  introduce `\r\n`. CodeMirror normalizes line breaks on insert by default, so
  offsets stay aligned; confirm the migration keeps `EditorState.lineSeparator`
  unset.
