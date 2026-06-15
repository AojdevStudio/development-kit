# ADR-0004: Single-product platform-chrome slot (account/billing reachability)

- **Status:** Accepted
- **Date:** 2026-06-15
- **Deciders:** AojdevStudio
- **Relates to:** #63 (PR #65) single-product shell mode; #66 (this decision); ADR-0001 (authority split)

## Context

The single-product shell mode added in #63 (PR #65) lets a sole-product app give
its product the entire root surface, hiding all generic chrome: the product nav
*and* the platform account/billing panels (`MePanel`, `BillingPanel`,
`AdvancedReportPanel`). That is the right default for "the product owns the root,"
and #63 deliberately did **not** invent billing UI (per the repo rule against
inventing billing/entitlement behavior).

But a real single-product SaaS app (e.g. OrinSync) still needs account/subscription
affordances (sign-in state, Upgrade, Manage billing) reachable somewhere. Left
as-is, every single-product app would re-surface those itself, risking the
per-product duplication the product-module seam exists to avoid. PR #65 left
`ShellLayout.showPlatformChrome` in place as the seam for this follow-up, but in
single-product mode it was a dead flag: the shell returned `<Root />` before the
flag was ever read.

The cross-vendor adversarial review of #63 (Forge / GPT-5.5) surfaced this gap.

## Decision

Adopt the **platform-chrome slot** (issue #66, option 1). In single-product mode,
`ShellConfig.showPlatformChrome` is now a live, opt-in flag that drives a minimal,
kit-provided account/billing slot:

- `resolveShellLayout` sets `ShellLayout.showPlatformChrome` from
  `config.showPlatformChrome ?? false` in single-product mode. It stays **off by
  default**, preserving #63's "product IS the app" default.
- When opted in, `<Shell>` renders `PlatformChromeSlot` alongside the product root.
  The slot composes the **existing, authority-backed** `MePanel` (account) and
  `BillingPanel` (Upgrade / Manage billing). It places existing panels, it does
  **not** invent any billing UI.
- The slot is **account + billing only**. `AdvancedReportPanel` is a product
  reporting feature, not platform chrome, and is not surfaced here.
- Multi-product mode is unchanged: it always renders platform chrome; the config
  field is a single-product concern.

This resolves the issue's either/or: `showPlatformChrome` now **drives a real
surface** rather than being removed.

## Consequences

- **Positive:** a single-product SaaS app reaches account/billing by setting one
  config flag, with no foundation fork and no duplicated billing logic, so billing
  chrome stays centralized in the kit. The seam PR #65 left in place is now real and
  unit-tested at the resolver (the kit's tested surface, since vitest runs in node
  with no DOM and `<Shell>` stays a thin consumer).
- **Negative / trade-offs:** the slot's placement is a single fixed region (product
  root, then the account/billing aside). Apps wanting bespoke placement of
  account/billing within their own layout still compose the panels themselves; the
  slot covers the common "make billing reachable" case, not arbitrary layout.
- **Follow-ups:** real auth replaces the walking-skeleton `DEV_TOKEN`; richer slot
  presentation (e.g. a collapsed account menu) if product feedback warrants.

## Alternatives considered

- **Composable platform panels only** (option 2): export `MePanel`/`BillingPanel`
  as primitives and let each product compose them. Rejected as the default because
  it pushes the integration burden onto every single-product app and reinvites the
  per-product duplication the seam exists to avoid. The panels remain importable, so
  this path stays open for apps that want bespoke placement.
- **Product-owned + remove the seam** (option 3): document that single-product apps
  surface billing themselves and delete `showPlatformChrome`. Rejected because it
  leaves the reachability gap open and discards the seam PR #65 deliberately
  preserved.
