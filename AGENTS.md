# kxs — AGENTS.md

## Project Overview
Tauri 2 + Svelte 5 + Rust desktop app for Kubernetes cluster management. Rust workspace with three crates.

## Structure
```
kxs/
├── src/                    # Svelte 5 frontend (TypeScript)
├── src-tauri/              # Tauri app, IPC bridge
├── crates/
│   ├── kxs-core/           # kubeconfig parsing, store, watcher
│   └── kxs-cluster/        # K8s operations (pods, logs, exec, port-forward, metrics, resources)
```

## Commands

### Frontend (from repo root)
```bash
npm run dev       # Vite dev server (port 1420)
npm run build     # Production build to ../dist
npm run check     # svelte-check (typecheck)
npm run test      # vitest run
npm run tauri     # Tauri CLI (dev, build, etc.)
```

### Rust (from repo root)
```bash
cargo build       # Build all workspace members
cargo test        # Test all crates
cargo check       # Type check all crates
cargo fmt         # Format (nightly)
cargo clippy      # Lint
```

### Combined dev
```bash
npm run tauri dev  # Runs `npm run dev` then starts Tauri
```

## Key Architecture
- **Frontend → IPC → Rust**: All K8s operations go through Tauri commands in `src-tauri/src/ipc.rs` and `src-tauri/src/cluster_ipc.rs`
- **kubeconfig**: Loaded at startup from standard paths (`~/.kube/config`, `KUBECONFIG` env). File watcher auto-reloads on changes.
- **Sessions**: Per-context cluster connections managed in `cluster_ipc::Sessions`, reused across commands.
- **State**: `AppState` holds `KubeconfigStore` (mutex) + warnings.

## Testing
- **Frontend**: `npm run test` (Vitest, `src/**/*.test.ts`)
- **Rust**: `cargo test` (unit tests in each crate)
- No integration/E2E tests configured.

## Conventions
- Svelte 5 runes (`$state`, `$derived`, `$effect`)
- TypeScript strict mode, verbatimModuleSyntax
- Rust 2021 edition, workspace resolver v2
- Tauri 2, CSP disabled
- `camelCase` for IPC payloads (serde `rename_all = "camelCase"`)

## Common Gotchas
- Frontend dev server **must** run on port 1420 (configured in `vite.config.ts` and `tauri.conf.json`)
- Run `npm run check` before committing — catches Svelte/TS errors
- Rust crates use path deps: `kxs-core` → `kxs-cluster` → `src-tauri`
- Kubeconfig watcher debounces at 300ms; redundant reloads are harmless
- No pre-commit hooks configured; run checks manually