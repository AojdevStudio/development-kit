# Migrations

Two separate migration trees, one per database, reflecting the authority split
(ADR-0001):

- `postgres/` — **cloud authority state.** Durable SaaS records: users, accounts,
  memberships, Stripe mappings, subscriptions, entitlements, license metadata,
  webhook event records, usage, audit events. Owned by `services/api`.
- `sqlite/` — **local product state.** Drafts, sync queues, cached reads, local
  indexes, non-authoritative app data. Owned by the desktop app. Local SQLite
  must never store authoritative billing status or subscription truth.

## Conventions

- Files are ordered, append-only, and named `NNNN_description.sql`
  (e.g. `0001_init.sql`). Never edit a migration that has shipped.
- Destructive or irreversible migrations are flagged for human review before
  merge (see `CLAUDE.md` → "Stop / escalate").
- The walking skeleton ships only the initial bootstrap migrations. The full
  authority schema (the data model in
  `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`) lands in its own issues.
