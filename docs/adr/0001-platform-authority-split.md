# ADR-0001: Platform authority split (client vs cloud)

- **Status:** Accepted
- **Date:** 2026-06-05
- **Deciders:** AojdevStudio

## Context

The kit must let many desktop SaaS products — including healthcare and finance
apps — reuse one trusted foundation. The single biggest risk is the client
becoming the billing or permission authority, or shipping secrets to user
machines. This decision is the spine every product depends on and is detailed in
`docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`.

## Decision

Enforce a hard, non-negotiable responsibility split:

- The **Tauri app** runs the product experience, owns local SQLite state, and
  *verifies* signed license tokens. It never decides payment status, holds
  secrets, or talks directly to Postgres.
- The **cloud Rust/Axum backend** owns identity, authorization, entitlement
  calculation, Stripe integration, and license-token *issuance*.
- **Postgres** stores durable SaaS authority state.
- **Stripe** owns the billing lifecycle; webhook processing is idempotent.

Feature gating is enforced at the UI, Tauri command, and backend layers. React
gating is UX only, never security. No Stripe/DB/webhook/signing secrets ship in
the desktop app.

## Consequences

- **Positive:** products start at the domain layer; security and billing
  authority are correct by default; regulated-domain audit/sensitivity needs are
  structurally supported.
- **Negative / trade-offs:** every paid action needs backend-backed checks and a
  license/entitlement round-trip (mitigated by short-lived offline license
  tokens); more moving parts than a client-only app.
- **Follow-ups:** entitlement engine, license service, billing service, feature
  gate service, sync service, and their test surfaces (see Goal PRD).

## Alternatives considered

- **Client-authoritative billing/gating** — rejected: trivially bypassable on a
  user's machine; unacceptable for regulated domains.
- **React-only feature gating** — rejected: presentation layer, not security.
- **Direct DB access from the desktop app** — rejected: leaks credentials and
  bypasses backend authority decisions.
