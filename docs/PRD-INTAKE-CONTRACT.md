# PRD Intake Contract for Tauri Desktop SaaS Apps

## Purpose

This document defines the required PRD format for any SaaS idea that will be plugged into the reusable Tauri desktop SaaS development kit.

The goal is to let a coding agent read a product PRD and immediately know how to add the product-specific layer without changing the platform spine.

The platform spine is fixed:

```text
Tauri v2 desktop app
React + Vite UI
Rust Tauri command layer
Local SQLite product-state database
Cloud Rust backend API
Cloud Postgres database
Stripe Billing, Checkout, Customer Portal, and webhooks
Signed short-lived license tokens
Feature-gated tiers
```

Every product PRD must describe only the product-specific layer:

```text
Domain model
Workflows
Roles
Feature tiers
Local data
Cloud data
Sync rules
Screens
Reports
Compliance boundaries
Acceptance tests
```

## Agent Contract

When a coding agent receives a product PRD, it must:

1. Preserve the standard architecture split from `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md`.
2. Treat the cloud Rust backend as the authority for identity, subscriptions, entitlements, and license issuance.
3. Treat local SQLite as the durable local product-state store, not as the billing authority.
4. Convert product tiers into explicit feature keys and limits.
5. Add product workflows behind React UI, Tauri Rust command guards, and backend authorization where needed.
6. Add local SQLite tables for offline-capable product state.
7. Add cloud Postgres tables for SaaS authority state and synced/shared product state.
8. Add tests for behavior, gates, sync, license handling, and domain rules.
9. Refuse to implement product behavior that would put Stripe secrets, database credentials, or authoritative billing decisions into the Tauri app.

## Required PRD Format

Every product PRD must use the sections below. If a section is not applicable, it must say `Not applicable` and explain why.

### 1. Product Summary

State the product in one paragraph.

Required fields:

```text
Product name:
Domain:
Primary customer:
Primary user:
Main job-to-be-done:
Regulated domain: yes/no
Expected offline use: yes/no
Expected team use: yes/no
```

### 2. Target Users and Roles

List every user type and what they are allowed to do.

Required table:

```text
Role | Description | Key permissions | Restrictions
```

Examples:

```text
Owner
Admin
Clinician
Analyst
Reviewer
Billing manager
Read-only user
```

### 3. Product Workflows

Describe each workflow as an ordered sequence.

Required format:

```text
Workflow name:
Actor:
Entry point:
Steps:
Successful outcome:
Failure states:
Offline behavior:
Audit requirements:
Feature gate:
```

Each workflow must identify whether it is:

```text
Local-only
Cloud-backed
Hybrid local/cloud
Billing/account workflow
```

### 4. Domain Model

Define the core product entities and relationships.

Required table:

```text
Entity | Description | Owned by | Local SQLite? | Cloud Postgres? | Synced? | Sensitive?
```

Rules:

- Use domain language, not generic placeholders.
- Identify parent-child relationships.
- Identify immutable records.
- Identify records that need audit history.
- Identify records that can be deleted versus archived.

### 5. Data Classification

Every meaningful data category must be classified.

Required table:

```text
Data category | Examples | Sensitivity | Local storage | Cloud storage | Encryption need | Retention rule
```

Sensitivity values:

```text
Public
Internal
Customer confidential
Financial
Healthcare/PHI
Authentication secret
Billing secret
```

Hard rules:

- Stripe secret keys are never local.
- Database credentials are never local.
- Billing authority state is never local-only.
- Healthcare and finance products must explicitly identify sensitive records and audit requirements.

### 6. Local SQLite Requirements

Define what the installed desktop app must store locally.

Required fields:

```text
Local tables:
Local indexes:
Draft state:
Offline work queue:
Sync queue:
Conflict candidates:
Local search needs:
Data retention:
Local purge behavior:
```

The PRD must say which local records are:

```text
Authoritative local product state
Cached cloud state
Pending sync state
Derived/indexed state
Temporary draft state
```

Local SQLite may be authoritative for local product work. It must never be authoritative for subscription status, paid access, or Stripe billing state.

### 7. Cloud Postgres Requirements

Define what the cloud database must store for this product.

Required fields:

```text
Cloud tables:
Account-scoped records:
User-scoped records:
Shared/team records:
Synced records:
Server-only records:
Audit records:
Usage records:
```

The PRD must identify which records are needed for:

```text
Authorization
Collaboration
Billing limits
Usage metering
Auditability
Recovery
Cross-device sync
```

### 8. Feature Tiers and Entitlements

Define plans as feature access, not only plan names.

Required table:

```text
Feature key | Free trial | Basic | Pro | Team | Enterprise | Limit type | Enforcement layer
```

Feature keys must be stable identifiers:

```text
export_pdf
cloud_sync
advanced_reports
team_members
max_projects
api_access
```

Enforcement layer values:

```text
React UI
Tauri Rust command
Cloud backend
Stripe/backend billing flow
```

Hard rules:

- React-only enforcement is not allowed for paid features.
- Local paid features must be guarded by Tauri Rust commands.
- Server-backed paid features must be guarded by the cloud Rust backend.
- Trial access must map to explicit entitlements.
- Downgrades must define what happens to over-limit data.

### 9. Trial and Billing Behavior

Define the business model.

Required fields:

```text
Trial length:
Requires card upfront: yes/no
Plans:
Stripe Products needed:
Stripe Prices needed:
Upgrade behavior:
Downgrade behavior:
Cancellation behavior:
Failed payment behavior:
Refund behavior:
Grace period:
Offline license duration:
```

The PRD must state what the user can do in each subscription state:

```text
trialing
active
past_due
canceled
unpaid
expired_offline_license
```

### 10. Offline and Sync Behavior

Define how the desktop app behaves without network access.

Required fields:

```text
Features available offline:
Features disabled offline:
Maximum offline duration:
License refresh requirement:
Sync trigger:
Conflict strategy:
Retry strategy:
User-visible sync states:
```

Conflict strategies:

```text
Last writer wins
Manual resolution
Server authoritative
Local authoritative until review
Append-only event log
```

Healthcare and finance products must prefer auditable conflict handling over silent overwrite.

### 11. Security, Privacy, and Compliance

Define product-specific security requirements.

Required fields:

```text
Regulatory context:
Sensitive data categories:
Local encryption requirements:
Transport requirements:
Audit log requirements:
Role-based access requirements:
Data export restrictions:
Data deletion requirements:
Admin access restrictions:
Support access restrictions:
```

For healthcare or finance products, the PRD must explicitly identify:

```text
PHI or financial data boundaries
Audit trails
Data retention
Access review needs
Export controls
Incident-relevant logs
```

### 12. UI Surface

List required screens and primary states.

Required table:

```text
Screen | Primary actor | Purpose | Data shown | Actions | Empty state | Error state | Feature gate
```

The UI should be dense, operational, and workflow-focused for healthcare, finance, and other professional tools. Avoid marketing-first layouts inside the app shell.

### 13. Reports, Exports, and Integrations

Define output and integration requirements.

Required fields:

```text
Reports:
Exports:
Imports:
External APIs:
File formats:
Scheduled jobs:
Manual review points:
Feature gates:
```

Each report/export must define:

```text
Source data
Filters
Format
Permission required
Audit entry required: yes/no
```

### 14. Acceptance Tests

List acceptance tests in user-facing terms.

Required format:

```text
Given <state>
When <action>
Then <observable result>
And <authorization/sync/audit/billing result>
```

Every PRD must include tests for:

```text
Happy path workflow
Permission denial
Tier-gated feature denial
Trial access
Downgrade behavior
Offline behavior
Sync behavior
Local SQLite persistence
Backend authorization
```

### 15. Out of Scope

State what must not be built in the first implementation.

Examples:

```text
Mobile app
Public web app
Custom payment collection UI
Direct database access from desktop
Admin back office
Enterprise SSO
Full compliance certification
```

### 16. Agent Implementation Notes

Give direct instructions to the coding agent.

Required fields:

```text
Preferred module boundaries:
Domain terms to preserve:
Deep modules to extract:
Known risky areas:
Expected tests:
Mock data needed:
Seed data needed:
Manual QA path:
```

The PRD must explicitly name the feature keys, workflows, and domain modules the agent should create.

## PRD Completeness Checklist

A product PRD is ready for implementation only when all of the following are true:

- Product roles are defined.
- Workflows are step-by-step.
- Domain entities are named.
- Local SQLite state is classified.
- Cloud Postgres state is classified.
- Feature tiers are mapped to feature keys.
- Billing states are defined.
- Offline behavior is defined.
- Sync behavior is defined.
- Security and compliance requirements are stated.
- UI screens are listed.
- Acceptance tests are written.
- Out-of-scope items are explicit.
- No authoritative billing decision depends on local-only state.

## Agent Refusal Conditions

A coding agent must stop and ask for clarification before implementation if:

- Feature tiers are missing.
- The PRD asks the Tauri app to connect directly to Postgres.
- The PRD asks for Stripe secret keys in the desktop app.
- Sensitive healthcare or finance data is mentioned but storage and audit rules are absent.
- Offline behavior is required but sync/conflict rules are missing.
- A paid feature is specified without an enforcement layer.
- A downgrade path is missing for limited resources.

## Minimal Product PRD Template

Copy this template for each new SaaS idea:

```markdown
# <Product Name> PRD

## 1. Product Summary

Product name:
Domain:
Primary customer:
Primary user:
Main job-to-be-done:
Regulated domain:
Expected offline use:
Expected team use:

## 2. Target Users and Roles

| Role | Description | Key permissions | Restrictions |
| --- | --- | --- | --- |

## 3. Product Workflows

### <Workflow Name>

Workflow name:
Actor:
Entry point:
Steps:
Successful outcome:
Failure states:
Offline behavior:
Audit requirements:
Feature gate:

## 4. Domain Model

| Entity | Description | Owned by | Local SQLite? | Cloud Postgres? | Synced? | Sensitive? |
| --- | --- | --- | --- | --- | --- | --- |

## 5. Data Classification

| Data category | Examples | Sensitivity | Local storage | Cloud storage | Encryption need | Retention rule |
| --- | --- | --- | --- | --- | --- | --- |

## 6. Local SQLite Requirements

Local tables:
Local indexes:
Draft state:
Offline work queue:
Sync queue:
Conflict candidates:
Local search needs:
Data retention:
Local purge behavior:

## 7. Cloud Postgres Requirements

Cloud tables:
Account-scoped records:
User-scoped records:
Shared/team records:
Synced records:
Server-only records:
Audit records:
Usage records:

## 8. Feature Tiers and Entitlements

| Feature key | Free trial | Basic | Pro | Team | Enterprise | Limit type | Enforcement layer |
| --- | --- | --- | --- | --- | --- | --- | --- |

## 9. Trial and Billing Behavior

Trial length:
Requires card upfront:
Plans:
Stripe Products needed:
Stripe Prices needed:
Upgrade behavior:
Downgrade behavior:
Cancellation behavior:
Failed payment behavior:
Refund behavior:
Grace period:
Offline license duration:

## 10. Offline and Sync Behavior

Features available offline:
Features disabled offline:
Maximum offline duration:
License refresh requirement:
Sync trigger:
Conflict strategy:
Retry strategy:
User-visible sync states:

## 11. Security, Privacy, and Compliance

Regulatory context:
Sensitive data categories:
Local encryption requirements:
Transport requirements:
Audit log requirements:
Role-based access requirements:
Data export restrictions:
Data deletion requirements:
Admin access restrictions:
Support access restrictions:

## 12. UI Surface

| Screen | Primary actor | Purpose | Data shown | Actions | Empty state | Error state | Feature gate |
| --- | --- | --- | --- | --- | --- | --- | --- |

## 13. Reports, Exports, and Integrations

Reports:
Exports:
Imports:
External APIs:
File formats:
Scheduled jobs:
Manual review points:
Feature gates:

## 14. Acceptance Tests

1. Given ...
   When ...
   Then ...
   And ...

## 15. Out of Scope

## 16. Agent Implementation Notes

Preferred module boundaries:
Domain terms to preserve:
Deep modules to extract:
Known risky areas:
Expected tests:
Mock data needed:
Seed data needed:
Manual QA path:
```
