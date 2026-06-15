# Development Kit — Tauri Desktop SaaS Starter

Reusable, agent-buildable foundation for desktop SaaS apps: Tauri v2 + React/Vite + Rust + local SQLite on the client; a cloud Rust/Axum backend + Postgres + Stripe Billing for authority. One trusted platform spine; product features plug in from a PRD without re-litigating the infrastructure.

## Read before building (source of truth)

- `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md` — the authority model and non-negotiables.
- `docs/DEVELOPMENT-KIT-GOAL-PRD.md` — the Persistent Build Goal, scope, deep modules, API surface, testing matrix, and acceptance criteria ("done" is defined here).
- `docs/PRD-INTAKE-CONTRACT.md` — how a new product PRD plugs into the kit.
- `docs/PRODUCT-MODULE-SEAM.md`: the product-module seam (`BackendModule` + `LocalModule` traits, plus `ProductModuleMeta`) for how a product plugs into all six dimensions (workflows, screens, tables, commands, feature keys, tests) without editing the foundation. See `docs/adr/0002-capability-crates-and-product-seam.md`.

## Target workspace layout

From the architecture doc — folder names may vary, the separation must not:

- `apps/desktop` — Tauri v2, React, Vite, Rust commands
- `services/api` — Rust/Axum backend; auth, Stripe, entitlement + license services
- `crates/shared` — shared Rust types, entitlement models, feature-key enums
- `migrations` — Postgres schema

## Verification — "done means"

Tooling: Cargo (Rust), Bun (frontend). The full required surfaces are in the Goal PRD ("Testing Decisions"); once code exists, a change ships only when its relevant gates pass. Critical gates:

- `cargo fmt`, `cargo clippy -D warnings`, Rust workspace tests
- Frontend lint/type-check, tests, build
- SQLite + Postgres migration checks
- Stripe webhook fixture tests, entitlement tests, license-token sign/verify tests, feature-gate tests, sync/offline tests

Tests target external behavior and authority boundaries, not private internals.

## Stop / escalate (don't improvise)

- A product PRD missing roles, feature tiers, data classification, offline behavior, sync rules, or acceptance tests → **stop and request clarification.** Do not invent risky billing/entitlement behavior.
- Destructive/irreversible migrations or security-sensitive entitlement/license/webhook code → flag for human review before merging.
- If blocked, stop and report: attempted paths, evidence gathered, the blocker, and the exact input or external-service config needed to continue.
- Healthcare/finance are first-class targets: treat local + cloud audit events, sensitive-data classification, and controlled exports as required, not optional.

## Working with fast-moving dependencies

Rust crates, Tauri v2, Stripe, React, and Vite all evolve past model training cutoffs — do not trust pretrained API memory for integration code. Verify against local sources first: `opensrc/` (see `~/AGENTS.md`), `node_modules`, and pinned crate versions (`cargo doc`). Fetch more with `bunx opensrc <pkg>` (or `pypi:` / `crates:` prefixes).

## Tooling

CodeGraph (`codegraph_`* MCP) for structural questions — definitions, callers, call paths, impact. Prefer it over grep for symbols. Guide + git rules: `docs/CODEGRAPH.md`. If not initialized, offer `codegraph init -i`.

## Project tracking

Primary tracker: GitHub Issues — [AojdevStudio/development-kit](https://github.com/AojdevStudio/development-kit).

## Agent / model workflow

Anthropic-first stack (per deep-research evaluation): Claude Sonnet as the default implementation model, escalating to Claude Opus for long-horizon refactors, recovery from stuck states, and final review of security-sensitive changes. When PAI orchestrates work in this repo, a cross-vendor `model:gpt-5.5` lane (Forge / Codex) handles adversarial review of authority-critical changes (see `docs/agents/triage-labels.md`). `AGENTS.md` is a symlink to this file — edit `CLAUDE.md`, never maintain a divergent copy.

## Agent skills

Operational config consumed by the engineering skills (issue/PR/triage/context workflows):

- **Issue tracker** — GitHub Issues (`AojdevStudio/development-kit`) via `gh`. See `docs/agents/issue-tracker.md`.
- **Agent workflow** — GitHub Issues + linked Project board (`development-kit Backlog`, #6) + PRs into `main`; self-review, fetch review comments, summarize automated reviews, manage releases on request, run the code-simplify agent at full task completion. See `docs/agents/workflow.md`.
- **Triage labels** — five canonical roles (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`) plus a `model:*` routing dimension (`model:opus-4.8`, `model:sonnet-4.6`, `model:gpt-5.5`). See `docs/agents/triage-labels.md`.
- **Project context docs** — single-context layout (`CONTEXT.md`, `PRODUCT.md`, `DESIGN.md`, `docs/adr/` at root). See `docs/agents/domain.md`.