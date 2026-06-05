# CONTEXT — Development Kit

Domain language and the platform authority model for this Tauri desktop SaaS
starter kit. Use these terms verbatim in issues, PRs, tests, and refactor
proposals. Canonical source docs: `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`,
`docs/DEVELOPMENT-KIT-GOAL-PRD.md`, `docs/PRD-INTAKE-CONTRACT.md`.

## What this is

A reusable foundation for building serious desktop SaaS apps (healthcare,
finance, and other logic-heavy domains) on one trusted spine. A coding agent
plugs a new product in from a PRD by adding domain modules — without changing the
platform authority model.

## The authority split (non-negotiable)

| Layer | Owns | Never |
| ----- | ---- | ----- |
| **Tauri app** (desktop client) | Product experience, local workflow logic, local SQLite state, license-token *verification*, local feature guards | Deciding payment status; holding Stripe/DB/webhook/signing secrets; direct DB access |
| **Cloud Rust backend** (Axum) | Identity, authorization, entitlement calculation, Stripe integration, license-token *issuance*, server-side gates | — |
| **Cloud Postgres** | Durable SaaS authority state: users, accounts, subscriptions, entitlements, license metadata, audit events | Storing non-authoritative local app state |
| **Stripe** | Billing lifecycle: checkout, subscriptions, trials, invoices, cancellations, webhooks | Being bypassed by local billing logic |

## Glossary

- **Platform spine** — the fixed, reusable infrastructure (auth, billing, entitlements, licensing, sync) shared by every product built on the kit.
- **Product layer** — the per-product domain modules, screens, schema, commands, and feature keys added from a PRD.
- **Entitlement** — the app-facing expression of a user's paid access, computed by the backend from plan + subscription state + trial + usage + account context.
- **Feature key** — a stable, explicit identifier for a gated capability; shared across React, Tauri commands, and backend authorization. Not a plan name.
- **Feature gate** — a check that a feature key is allowed, enforced at the UI, Tauri command, and backend layers. React gating is UX only, never security.
- **License token** — a short-lived, signed token issued by the backend and verified inside the Tauri Rust layer to permit bounded offline paid access.
- **Sync queue** — local SQLite-backed queue of offline changes with retry and conflict reconciliation policy.
- **Authority boundary** — the rule that the client never becomes billing/permission authority; all authority decisions pass through the backend.
- **PRD intake contract** — the required shape of a product PRD (`docs/PRD-INTAKE-CONTRACT.md`) that lets an agent implement a product without re-litigating infrastructure.

## Done means

Behavior- and boundary-focused verification (see Goal PRD "Testing Decisions"):
Rust fmt/clippy/tests, frontend lint/type/test/build, SQLite + Postgres
migrations, Stripe webhook fixtures, entitlement/license/feature-gate/sync
tests, and a sample product module generated from the intake contract — with no
secrets in the desktop artifact.
