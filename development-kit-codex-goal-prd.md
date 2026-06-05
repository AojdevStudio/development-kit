# Codex Goal PRD: Tauri Desktop SaaS Development Kit

## Codex Goal

Use this as the persistent Codex Goal when building the development kit:

```text
/goal Build a testable, reusable Tauri desktop SaaS development kit that implements the standard architecture in docs/tauri-stripe-saas-architecture.md and accepts product PRDs that follow docs/prd-intake-contract.md. The finished kit must let a coding agent plug in a new SaaS product by adding domain modules, local SQLite schema, cloud Postgres schema, React screens, Tauri commands, entitlement mappings, and tests without changing the platform authority model. Verify completion with passing Rust workspace tests, frontend tests, lint/type checks, database migration checks, Stripe webhook fixture tests, license-token tests, feature-gate tests, sync/offline tests, and a sample product module generated from the PRD intake contract. Preserve the hard split: Tauri runs the product experience, local SQLite stores local product state, the cloud Rust backend decides identity/payment/entitlements/licenses, Postgres stores durable SaaS authority state, and Stripe handles billing lifecycle. Do not ship Stripe secrets, database credentials, webhook secrets, or license-signing private keys in the Tauri app. Between iterations, inspect failing tests or missing acceptance criteria, make the smallest architectural change that advances the kit, rerun the relevant verification, and record what remains. If blocked, stop with the attempted paths, evidence gathered, blocker, and exact input or external service configuration needed to continue.
```

## Problem Statement

The user wants to repeatedly build serious desktop SaaS applications in regulated and logic-heavy domains such as healthcare and finance. Each app should use the same trusted foundation: Tauri v2, React, Vite, Rust, local SQLite, a cloud Rust backend, Postgres, Stripe Billing, entitlements, short-lived licenses, and feature-gated subscription tiers.

Without a reusable development kit, each new product idea forces the coding agent to rediscover the same architecture, rebuild the same billing/auth/licensing/sync foundation, and risk violating the core authority boundary. The user needs a starter kit that is reusable, testable, and strict enough that a product PRD can be plugged into it by an agent without re-litigating the infrastructure.

## Solution

Build a reusable Tauri desktop SaaS development kit with a fixed platform spine and a pluggable product layer.

The platform spine must include:

- Tauri v2 desktop shell.
- React + Vite frontend.
- Rust Tauri command layer.
- Local SQLite database for durable local product state.
- Cloud Rust API.
- Postgres database migrations.
- Authentication boundary.
- Stripe Checkout flow.
- Stripe Customer Portal flow.
- Stripe webhook ingestion.
- Subscription and entitlement services.
- Signed short-lived license token service.
- Feature gate system.
- Offline sync queue foundation.
- Audit/event logging foundation.
- Shared Rust types for app/backend contracts.
- Test harnesses for platform behavior.
- Agent documentation and PRD templates.

The product layer must be added from a PRD that follows `docs/prd-intake-contract.md`. A coding agent should be able to add a new product by implementing domain models, workflows, UI screens, SQLite tables, Postgres tables, sync rules, feature keys, reports, and tests while preserving the platform spine.

## User Stories

1. As the product owner, I want one reusable starter kit, so that every new desktop SaaS product starts from the same trusted architecture.
2. As the product owner, I want coding agents to follow a strict PRD intake contract, so that product ideas are implemented consistently.
3. As the product owner, I want Tauri to run the desktop product experience, so that apps feel local, fast, and suitable for deep workflows.
4. As the product owner, I want local SQLite included by default, so that healthcare and finance apps can support durable local state, drafts, modules, queues, and offline workflows.
5. As the product owner, I want the cloud Rust backend to own identity, payment status, entitlements, and license issuance, so that local clients never become billing authority.
6. As the product owner, I want Postgres to store durable SaaS authority state, so that subscriptions, accounts, roles, entitlements, and audit records are reliable.
7. As the product owner, I want Stripe to handle billing lifecycle, so that checkout, subscriptions, trials, invoices, cancellations, and payment failures use a proven billing provider.
8. As a coding agent, I want the platform boundaries documented in the repo, so that I know which layer owns each responsibility.
9. As a coding agent, I want shared entitlement types, so that React, Tauri commands, and backend authorization use consistent feature keys.
10. As a coding agent, I want a PRD template, so that each new product idea arrives with roles, workflows, data model, tiers, offline behavior, and tests.
11. As a coding agent, I want example product modules, so that I can copy the expected implementation shape.
12. As a coding agent, I want migration conventions for SQLite and Postgres, so that local product state and cloud authority state evolve safely.
13. As a developer, I want local mock mode, so that the desktop app can run without live Stripe or production cloud services during development.
14. As a developer, I want Stripe webhook fixture tests, so that billing state changes can be verified without depending on live webhook delivery.
15. As a developer, I want license-token tests, so that offline feature access is cryptographically checked.
16. As a developer, I want feature-gate tests, so that tier leakage is caught before release.
17. As a developer, I want Tauri command gate tests, so that local paid actions cannot run without entitlements.
18. As a developer, I want backend authorization tests, so that server-backed paid actions cannot rely on the client to self-report access.
19. As a developer, I want SQLite persistence tests, so that local product workflows survive restart.
20. As a developer, I want sync queue tests, so that offline changes are retried and reconciled predictably.
21. As a developer, I want a sample product PRD implemented inside the kit, so that the entire intake-to-module workflow is proven.
22. As a billing admin, I want Customer Portal support, so that users manage payment methods, invoices, cancellations, and plan changes through Stripe.
23. As an end user, I want trial access to work cleanly, so that I can evaluate paid functionality without the app misrepresenting access.
24. As an end user, I want paid features to be clearly gated, so that I understand what my tier includes.
25. As an end user, I want offline access to continue for a bounded period, so that desktop work is not interrupted by short network outages.
26. As a finance or healthcare operator, I want audit-friendly local and cloud events, so that important actions can be reviewed.
27. As a security reviewer, I want no secrets in the desktop app, so that customer machines cannot expose Stripe, database, webhook, or signing credentials.
28. As a security reviewer, I want direct database access blocked from the desktop client, so that all cloud authority decisions pass through the backend.
29. As an agent reviewer, I want acceptance tests tied to the PRD, so that implementation can be judged by behavior instead of code shape alone.
30. As the product owner, I want the kit to be reusable across many SaaS ideas, so that each new product starts at the domain layer instead of infrastructure.

## Implementation Decisions

- The development kit will be a multi-part workspace with a desktop app, cloud API, shared Rust crates, database migrations, tests, templates, and agent documentation.
- The desktop app will use Tauri v2, React, Vite, Rust Tauri commands, and local SQLite.
- The cloud API will use Rust and Axum.
- The cloud database will use Postgres.
- Stripe integration will live only in the cloud backend.
- The desktop app will call the cloud backend through authenticated HTTPS API requests.
- The desktop app will not connect directly to Postgres.
- The desktop app will not include Stripe secret keys, database URLs, webhook secrets, or license-signing private keys.
- Entitlements will be represented as stable feature keys and limits, not only plan names.
- React may present gated UI, but React will never be the only enforcement layer for paid features.
- Tauri Rust commands will gate local paid actions.
- The cloud backend will gate server-backed paid actions.
- Local SQLite will store local product state, drafts, sync queues, cached reads, local indexes, and non-authoritative app data.
- Local SQLite will not store authoritative billing status or subscription truth.
- Postgres will store users, accounts, memberships, Stripe mappings, subscriptions, entitlements, license metadata, webhook event records, usage records, audit events, and synced/shared product state.
- Stripe Checkout will be used for signup, trial conversion, and upgrades.
- Stripe Customer Portal will be used for billing management.
- Stripe webhooks will update backend subscription and entitlement state.
- Webhook processing will be idempotent.
- Short-lived signed license tokens will support bounded offline paid access.
- The license token verifier will run inside the Tauri Rust layer.
- The license token issuer and signing private key will remain in the cloud backend.
- The kit will include a mock billing mode for local development and automated tests.
- The kit will include an example product module generated from the PRD intake contract.
- The kit will include an agent guide explaining how to convert a product PRD into implementation tasks.
- The kit will treat healthcare and finance apps as first-class targets, with explicit support for local state, audit events, sensitive data classification, and controlled exports.

## Deep Modules

The kit should extract these deep modules behind stable interfaces:

- Entitlement engine: maps plan, subscription state, trial state, usage, and account context into feature access.
- License service: issues, verifies, expires, and revokes short-lived desktop license tokens.
- Billing service: creates Checkout and Customer Portal sessions and reconciles Stripe webhook events.
- Auth/account service: resolves the current user, account, membership, and role.
- Feature gate service: checks whether a feature is allowed at the UI, Tauri command, and backend layers.
- Local data service: owns SQLite migrations, local repositories, drafts, indexes, and persistence behavior.
- Sync service: owns offline queueing, retry, conflict detection, and reconciliation policy.
- Audit/event service: records important local and cloud actions in a reviewable format.
- Product module interface: defines how a PRD-driven domain module plugs in workflows, screens, tables, commands, feature keys, and tests.

## API and Contract Decisions

The cloud backend must expose at least:

```text
GET  /me
GET  /me/entitlements
POST /license/refresh
POST /billing/checkout
POST /billing/portal
POST /stripe/webhook
```

The desktop app must support at least:

```text
Load current user/account state
Load entitlements
Refresh offline license
Verify local license token
Gate local commands
Read/write local SQLite product state
Queue offline sync work
Display billing upgrade/portal flows
```

The product module contract must support:

```text
Feature keys
Domain entities
Local SQLite schema
Cloud Postgres schema
Tauri commands
React routes/screens
Backend routes, if needed
Sync rules
Reports/exports
Acceptance tests
```

## Testing Decisions

Tests must focus on external behavior and authority boundaries, not private implementation details.

Required verification surfaces:

- Rust workspace tests pass.
- Rust formatting check passes.
- Rust linting passes.
- Frontend tests pass.
- Frontend lint/type checks pass.
- Frontend build passes.
- SQLite migration tests pass.
- Postgres migration checks pass.
- Stripe webhook fixture tests pass.
- Entitlement calculation tests pass.
- License token signing and verification tests pass.
- Tauri command feature-gate tests pass.
- Backend authorization tests pass.
- Offline sync queue tests pass.
- PRD intake example product tests pass.

Minimum behavioral test cases:

1. Trial user receives trial entitlements.
2. Active paid user receives paid entitlements.
3. Past-due user loses or degrades paid access according to policy.
4. Canceled user loses paid access at the correct time.
5. Downgraded user cannot access over-tier features.
6. React shows gated UI for unavailable paid features.
7. Tauri command denies a local paid action without entitlement.
8. Backend denies a server-backed paid action without entitlement.
9. Desktop app verifies a valid signed license token.
10. Desktop app rejects an expired license token.
11. Desktop app rejects a tampered license token.
12. Stripe webhook processing is idempotent.
13. Local SQLite product state persists across restart.
14. Offline work is queued and retried.
15. Sync conflict behavior matches the product policy.
16. No desktop build artifact contains Stripe secrets or database credentials.

## Acceptance Criteria

The development kit is complete only when:

- The architecture document exists and is reflected in code boundaries.
- The PRD intake contract exists and is usable for new product ideas.
- A sample product PRD can be converted into a working module.
- The sample module includes local SQLite state.
- The sample module includes cloud Postgres state where appropriate.
- The sample module includes feature tiers and entitlement mappings.
- The sample module includes React UI screens.
- The sample module includes Tauri commands.
- The sample module includes backend routes where needed.
- The sample module includes tests proving gated access.
- The desktop app can run in local development mode.
- The cloud backend can run in local development mode.
- Billing flows can be exercised with Stripe mocks or fixtures.
- License tokens can be issued by the backend and verified by the desktop app.
- Offline behavior is represented by tests.
- Sync queue behavior is represented by tests.
- All required verification commands pass.
- The implementation does not violate the authority split.

## Out of Scope

- Building a specific production healthcare or finance product.
- Full compliance certification.
- Real production Stripe account setup.
- Production cloud deployment automation beyond local-ready configuration.
- Mobile applications.
- Public web application experience.
- Enterprise SSO.
- Multi-region infrastructure.
- Custom payment collection UI.
- Replacing Stripe with another billing provider.
- Direct database access from the Tauri app.

## Further Notes

The kit should be optimized for coding-agent use. Documentation should be written as operational instructions, not as marketing copy.

The starter kit must make the correct architecture the easiest path. A coding agent should not need to decide where billing authority lives, whether Stripe secrets go into Tauri, or whether React-only gating is enough. Those decisions are already made.

The product-specific work should begin after the PRD intake contract is complete. If a product PRD is missing roles, feature tiers, data classification, offline behavior, sync rules, or acceptance tests, the coding agent should stop and request clarification rather than inventing risky behavior.
