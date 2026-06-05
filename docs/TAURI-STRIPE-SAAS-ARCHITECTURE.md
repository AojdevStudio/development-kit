# Tauri Desktop SaaS Billing Architecture

## Purpose

This document defines the required architecture for a desktop SaaS app built with Tauri v2, React, Vite, Rust, a cloud Rust backend, a cloud database, and Stripe Billing.

The core principle is absolute:

```text
The shipped Tauri app runs the product experience.
The cloud Rust backend decides identity, payment status, permissions, and licenses.
The cloud database stores durable user, subscription, entitlement, and account state.
Stripe handles billing events, money movement, trials, invoices, cancellations, and subscription lifecycles.
```

The desktop app is never the billing authority. React is never the permission authority. Local files are never the subscription source of truth. Stripe secret keys are never shipped inside the Tauri app.

## Absolute Responsibility Split

### Tauri App

The Tauri app is the installed desktop client. It contains:

- React + Vite UI.
- Local Tauri Rust commands.
- Local product workflow logic.
- Local file access.
- Local settings.
- Local cache.
- Local SQLite for offline-capable product state, local modules, sync queues, drafts, cached reads, local indexes, and non-authoritative app data.
- Verification of signed short-lived license tokens.
- Local feature guards for actions that run on the user's machine.

The Tauri app runs the product experience. It does not decide whether a user has paid. It asks the cloud backend for entitlement state and enforces the returned permissions locally.

The Tauri app must only call public HTTPS API endpoints exposed by the cloud backend. It must never connect directly to the cloud database.

For healthcare, finance, and other logic-heavy desktop apps, local SQLite is part of the default app architecture. These products usually need durable local state for complex workflows, offline operation, draft work, local validation, cached domain data, audit-friendly local queues, and sync reconciliation.

### Cloud Rust Backend

The cloud Rust backend is the SaaS authority. It contains:

- Authentication handling.
- User/account/workspace lookup.
- Authorization decisions.
- Stripe Checkout Session creation.
- Stripe Customer Portal Session creation.
- Stripe webhook handling.
- Subscription state reconciliation.
- Entitlement calculation.
- License token issuance.
- Server-backed feature gates.
- API endpoints used by the Tauri app.

The cloud backend decides:

- Who the user is.
- Which account or workspace the user belongs to.
- Whether the user is in trial.
- Whether the user is actively subscribed.
- Which plan the account has.
- Which features and limits are enabled.
- Whether a paid action is allowed.
- Whether to issue or deny a fresh desktop license token.

The backend may be built with Axum when an HTTP API is needed. The backend owns all Stripe secret key usage.

### Cloud Database

The cloud database stores durable SaaS state. Use Postgres by default.

It contains records such as:

- Users.
- Accounts or workspaces.
- Memberships and roles.
- Stripe customer mappings.
- Stripe subscription mappings.
- Subscription status.
- Plan and tier state.
- Feature entitlements.
- Usage counters and limits.
- License token metadata.
- Webhook event processing records.

The cloud database is private infrastructure. The desktop app must never receive credentials for it and must never connect to it directly.

### Stripe

Stripe owns billing mechanics. It handles:

- Checkout.
- Payment methods.
- Subscriptions.
- Subscription trials.
- Recurring invoices.
- Failed payment handling.
- Plan changes.
- Cancellations.
- Customer billing portal.
- Billing lifecycle webhooks.
- Optional Stripe Billing Entitlements.

Stripe is the billing processor. The cloud backend translates Stripe billing state into application access state.

## Required Runtime Topology

```text
Tauri desktop app
  React + Vite UI
  Tauri Rust commands
      |
      | Authenticated HTTPS requests
      v
Cloud Rust backend API
      |
      | Private database connection
      v
Cloud Postgres database
      ^
      |
Stripe webhooks
      ^
      |
Stripe Billing, Checkout, Customer Portal
```

The Tauri app points to the cloud backend API, not the database.

## Trust Model

Anything shipped to the user's machine must be treated as inspectable and modifiable.

This includes:

- React UI code.
- Tauri Rust binary.
- Local SQLite databases.
- Local config files.
- Local license cache.
- Local feature flags.
- Network responses cached on disk.

Therefore:

- UI gating is convenience only.
- Local Rust gating is useful, but not the final authority for server-backed value.
- The backend is the authority for paid access.
- The database is the durable record.
- Stripe is the billing lifecycle source.

For valuable server-backed capabilities, every request must be authorized by the cloud backend at request time.

For local-only paid capabilities, use signed short-lived license tokens issued by the cloud backend and verified by the Tauri Rust layer.

Local SQLite can store product state, user work, workflow state, sync queues, cached domain records, and local indexes. Local SQLite must not store authoritative billing status, subscription truth, or unverified feature access.

## Billing Flow

### Signup or Upgrade

1. User clicks a plan in the Tauri app.
2. Tauri calls the cloud backend: `POST /billing/checkout`.
3. Backend authenticates the user.
4. Backend finds or creates the Stripe Customer for the user's account.
5. Backend creates a Stripe Checkout Session for the selected Price.
6. Backend returns the Checkout URL.
7. Tauri opens the Checkout URL in the system browser.
8. User completes checkout in Stripe.
9. Stripe sends webhook events to the cloud backend.
10. Backend records subscription state in Postgres.
11. Backend calculates entitlements.
12. Tauri refreshes entitlements from `GET /me/entitlements`.

### Billing Management

1. User clicks "Manage billing" in the Tauri app.
2. Tauri calls the cloud backend: `POST /billing/portal`.
3. Backend authenticates the user.
4. Backend creates a Stripe Customer Portal Session.
5. Backend returns the portal URL.
6. Tauri opens the portal URL in the system browser.
7. User manages payment methods, invoices, cancellation, or plan changes in Stripe.
8. Stripe sends webhook events.
9. Backend updates local subscription and entitlement state.

### Free Trial

Trials must be account-bound and backend-known. Do not implement the authoritative trial as only a local timestamp.

The preferred trial model is:

1. Trial is created through Stripe subscription trial settings.
2. Stripe owns the trial lifecycle.
3. Stripe sends trial and subscription webhooks.
4. Backend stores `trial_ends_at`, `status`, and plan state.
5. Backend returns trial entitlements to the Tauri app.
6. Tauri gates features using the entitlement snapshot or signed license token.

If a non-Stripe trial is needed, the backend still owns it. The trial must be tied to a user/account identity in the cloud database.

## Data Model

Use this as the baseline schema shape. Names can vary, but the responsibilities must remain.

```text
users
- id
- email
- created_at
- disabled_at

accounts
- id
- owner_user_id
- name
- created_at

memberships
- id
- account_id
- user_id
- role
- created_at

stripe_customers
- id
- account_id
- stripe_customer_id
- created_at

subscriptions
- id
- account_id
- stripe_subscription_id
- stripe_price_id
- plan_key
- status
- trial_ends_at
- current_period_ends_at
- cancel_at_period_end
- updated_at

entitlements
- id
- account_id
- feature_key
- enabled
- limit_value
- source
- updated_at

license_tokens
- id
- account_id
- user_id
- token_id
- issued_at
- expires_at
- revoked_at

stripe_webhook_events
- id
- stripe_event_id
- event_type
- processed_at
- processing_status
```

## Entitlements

Entitlements are the app-facing expression of a user's paid access.

The app should not reason directly from raw Stripe subscription objects. The backend should convert billing state into product permissions.

Example entitlement response:

```json
{
  "account_id": "acct_123",
  "plan": "pro",
  "status": "active",
  "trial": false,
  "features": {
    "export_pdf": true,
    "cloud_sync": true,
    "advanced_reports": true,
    "team_members": 5,
    "max_projects": 100
  },
  "license_expires_at": "2026-06-12T00:00:00Z"
}
```

Feature keys must be stable and explicit:

```text
export_pdf
cloud_sync
advanced_reports
team_members
max_projects
priority_support
api_access
```

Paid access must be represented as feature access, not only as plan names. Code should ask "does this account have `export_pdf`?" rather than "is this account Pro?" whenever possible.

## License Tokens for Desktop Offline Use

The desktop app may need to work offline. Offline support must not turn local state into billing authority.

Use signed short-lived entitlement tokens.

Token payload:

```json
{
  "token_id": "lic_123",
  "user_id": "user_123",
  "account_id": "acct_123",
  "plan": "pro",
  "features": {
    "export_pdf": true,
    "cloud_sync": true,
    "advanced_reports": true,
    "max_projects": 100
  },
  "issued_at": "2026-06-05T12:00:00Z",
  "expires_at": "2026-06-12T12:00:00Z"
}
```

Rules:

- Token is issued only by the cloud backend.
- Token is signed by the backend.
- Tauri Rust verifies the signature locally.
- Token expires regularly.
- Tauri refreshes the token when online.
- Expired token means premium local access is disabled or degraded.
- Revocation takes effect when the app next checks in or when the token expires.

Do not store the Stripe subscription object in the local app and treat it as proof of access.

## Feature Gating Rules

Every paid feature must be gated at the correct layer.

### React UI Gate

React may hide, disable, or label features based on entitlements.

React gating is for user experience only. It is not security.

### Tauri Rust Command Gate

Every local paid command must check entitlements before executing.

Example:

```rust
fn require_feature(entitlements: &Entitlements, feature: Feature) -> Result<(), AppError> {
    if entitlements.allows(feature) {
        Ok(())
    } else {
        Err(AppError::UpgradeRequired)
    }
}
```

### Cloud Backend Gate

Every server-backed paid feature must check authorization and entitlements on the backend.

Examples:

- Cloud sync.
- Team collaboration.
- AI usage.
- Hosted exports.
- API access.
- Account management.
- Server-side storage.
- Shared projects.

The backend must not rely on the client to honestly report its plan or features.

## Required API Surface

Minimum backend endpoints:

```text
GET  /me
GET  /me/entitlements
POST /license/refresh
POST /billing/checkout
POST /billing/portal
POST /stripe/webhook
```

Common additional endpoints:

```text
GET  /account
GET  /account/members
POST /account/members
DELETE /account/members/:id
GET  /usage
POST /usage/events
```

All non-webhook endpoints must require authentication. The Stripe webhook endpoint must verify Stripe's webhook signature.

## Stripe Integration Rules

Use Stripe Checkout for signup, paid conversion, and upgrades.

Use Stripe Customer Portal for customer-managed billing actions.

Use Stripe webhooks to update the cloud database.

Handle at least these webhook categories:

```text
checkout.session.completed
customer.subscription.created
customer.subscription.updated
customer.subscription.deleted
invoice.payment_succeeded
invoice.payment_failed
customer.subscription.trial_will_end
```

If using Stripe Billing Entitlements, the backend may consume Stripe entitlement updates and persist the resulting active feature set internally. The app should still read from the backend's entitlement endpoint, not directly from Stripe.

Webhook processing must be idempotent. Store processed Stripe event IDs.

## Agent Implementation Instructions

When implementing this architecture, do not flatten the cloud backend into the Tauri app.

Build these as separate concerns:

```text
apps/desktop
  Tauri v2
  React
  Vite
  Rust Tauri commands

services/api
  Rust HTTP backend
  Auth
  Stripe integration
  Entitlement service
  License token service

crates/shared
  Shared Rust types where useful
  Entitlement models
  Feature key enums

migrations
  Postgres schema
```

The exact folder names can vary, but the separation must remain.

The desktop app must receive:

- Public API base URL.
- Public auth configuration.
- Public app configuration.

The desktop app must not receive:

- Stripe secret key.
- Database URL.
- Webhook signing secret.
- Backend license signing private key.
- Any admin credential.

## Default Stack

Use this stack unless a project constraint explicitly requires otherwise:

```text
Desktop shell: Tauri v2
Desktop UI: React + Vite
Local desktop logic: Rust Tauri commands
Local desktop database: SQLite
Cloud API: Rust
HTTP framework: Axum
Database: Postgres
Billing: Stripe Billing, Checkout, Customer Portal, webhooks
Frontend tooling: Bun
Rust tooling: Cargo
```

Axum belongs in the cloud backend API. Tauri commands are the local desktop API surface.

## Non-Negotiables

- The Tauri app runs the product experience.
- The cloud Rust backend decides identity, payment status, permissions, and license issuance.
- The cloud database stores durable user, account, subscription, entitlement, and license state.
- Stripe handles payments, subscriptions, trials, invoices, cancellations, and webhook events.
- The Tauri app calls the backend API over HTTPS.
- The Tauri app never connects directly to the cloud database.
- The Tauri app never contains Stripe secret keys.
- Local SQLite is used for durable local product state, not authoritative billing state.
- React never acts as the only paid-feature gate.
- Local state never acts as the subscription source of truth.
- Every paid local command must be guarded in Tauri Rust.
- Every paid server-backed action must be guarded in the cloud backend.
- Offline paid access must use signed short-lived license tokens.

## One-Sentence Summary

Tauri is the client experience, the cloud Rust backend is the authority, Postgres is the durable record, and Stripe is the billing system.
