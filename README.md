# kxs

A Tauri 2 + Svelte 5 + Rust desktop app for Kubernetes cluster management.

## Overview

kxs is a desktop application that provides a UI for managing Kubernetes clusters. It loads kubeconfig files from standard locations (`~/.kube/config`, `KUBECONFIG` env), watches for changes, and provides an interface for common cluster operations.

## Architecture

- **Frontend**: Svelte 5 with TypeScript (runes: `$state`, `$derived`, `$effect`)
- **Backend**: Rust workspace with three crates:
  - `kxs-core` — kubeconfig parsing, store, file watcher
  - `kxs-cluster` — K8s operations (pods, logs, exec, port-forward, metrics, resources)
  - `src-tauri` — Tauri app, IPC bridge
- **Communication**: Frontend → Tauri IPC → Rust commands

## Features

- **Context management**: List, switch, ping, save, delete Kubernetes contexts
- **Pod operations**: Watch pods, list containers, stream logs
- **Resource management**: List resource kinds, list resources in table view, get YAML, get events, apply YAML, delete, scale, restart
- **Node operations**: Cordon/uncordon nodes
- **Exec**: Interactive terminal sessions in pods
- **Port forwarding**: Start/stop port forwards, list active forwards
- **Metrics**: Pod metrics (requires metrics-server)

## Development

### Prerequisites

- Node.js 20+
- Rust (stable)
- Tauri CLI (`cargo install tauri-cli`)

### Commands

**Frontend (from repo root):**
```bash
npm run dev       # Vite dev server (port 1420)
npm run build     # Production build to ../dist
npm run check     # svelte-check (typecheck)
npm run test      # vitest run
npm run tauri     # Tauri CLI (dev, build, etc.)
```

**Rust (from repo root):**
```bash
cargo build       # Build all workspace members
cargo test        # Test all crates
cargo check       # Type check all crates
cargo fmt         # Format (nightly)
cargo clippy      # Lint
```

**Combined dev:**
```bash
npm run tauri dev  # Runs `npm run dev` then starts Tauri
```

### Project Structure

```
kxs/
├── src/                    # Svelte 5 frontend (TypeScript)
│   ├── lib/
│   │   ├── components/     # Svelte components
│   │   ├── stores/         # Svelte 5 stores (runes)
│   │   └── *.ts            # Utilities, API, keybindings
│   ├── App.svelte
│   └── main.ts
├── src-tauri/              # Tauri app, IPC bridge
│   ├── src/
│   │   ├── main.rs         # Entry point
│   │   ├── ipc.rs          # Kubeconfig IPC commands
│   │   ├── cluster_ipc.rs  # Cluster operation IPC commands
│   │   └── watcher.rs      # Kubeconfig file watcher
│   └── Cargo.toml
├── crates/
│   ├── kxs-core/           # kubeconfig parsing, store, watcher
│   └── kxs-cluster/        # K8s operations
```

## Kubeconfig

Loaded at startup from:
- `~/.kube/config`
- `KUBECONFIG` environment variable (colon-separated paths)

File watcher auto-reloads on changes (300ms debounce).

## Testing

- **Frontend**: `npm run test` (Vitest, `src/**/*.test.ts`)
- **Rust**: `cargo test` (unit tests in each crate)

## Conventions

- Svelte 5 runes (`$state`, `$derived`, `$effect`)
- TypeScript strict mode, `verbatimModuleSyntax`
- Rust 2021 edition, workspace resolver v2
- Tauri 2, CSP disabled
- `camelCase` for IPC payloads (serde `rename_all = "camelCase"`)

## License

Private project.