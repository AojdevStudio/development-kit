# Development Kit — Tauri Desktop SaaS Starter

A reusable, agent-buildable foundation for desktop SaaS apps. Tauri v2 + React/Vite
+ Rust + local SQLite on the client; a cloud Rust/Axum backend + Postgres + Stripe
for authority. One trusted platform spine; product features plug in from a PRD
without re-litigating the infrastructure.

Read the source-of-truth docs first: `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`,
`docs/DEVELOPMENT-KIT-GOAL-PRD.md`, `docs/PRD-INTAKE-CONTRACT.md`, `CONTEXT.md`,
and the ADRs in `docs/adr/`.

## Workspace layout

```
crates/shared          Pure shared types: feature keys, entitlement + license DTOs (no heavy deps)
crates/license-verify  Public-key license-token VERIFY only — desktop-safe
crates/license-sign    Private-key license-token SIGN only — backend-only
services/api           Cloud Rust/Axum backend — the SaaS authority
apps/desktop           Tauri v2 + React + Vite desktop app (src-tauri/ = Rust)
migrations             Postgres (cloud authority) + SQLite (local) migration trees
xtask                  The single gate runner; mechanical ADR-0002 edge enforcement
```

## Authority boundary (ADR-0001 / ADR-0002)

The split is enforced as a **compile fact**, not a convention. `apps/desktop`
depends on `shared` + `license-verify` only — `license-sign`, `sqlx`, and Stripe
are absent from its dependency graph, so the client physically cannot issue
licenses, reach Postgres, or hold billing secrets. `cargo xtask edges` is the CI
backstop that fails the moment a forbidden edge is introduced.

## Prerequisites

- Rust (stable; pinned `rust-version` in `Cargo.toml`)
- [Bun](https://bun.sh) for the frontend
- Tauri system dependencies for your OS (see <https://v2.tauri.app/start/prerequisites/>)

## Run it

```bash
# Cloud API (serves GET /health)
cargo run -p api                 # http://127.0.0.1:8787/health  (override with API_PORT)

# Desktop app in dev (opens a window; starts Vite via beforeDevCommand)
cd apps/desktop && bun install && bun run tauri dev
```

## Verify it (the gate)

```bash
cargo xtask gate --scope all     # fmt + clippy + workspace tests + ADR-0002 edges + frontend
cargo xtask gate --scope rust    # Rust slice only
cargo xtask edges                # ADR-0002 crate-edge check only
```

The same `cargo xtask gate` runs locally and in CI (`.github/workflows/gate.yml`).
