# PRODUCT — Development Kit

Strategic context for product/UI/UX work. Read before designing screens, copy,
onboarding, or design-system changes. This file is strategic (who/why/what it
must not feel like); visual rules live in `DESIGN.md`.

> Seed stub — refine via `/grill-with-docs` or `impeccable teach` as real product
> decisions get made.

## Who this is for

Two distinct audiences:

1. **Kit consumers (primary):** developers and coding agents building desktop
   SaaS products on top of the spine. They need the correct architecture to be
   the easiest path and need to plug in a product from a PRD without deciding
   where billing authority lives.
2. **End users of the built products (secondary):** operators in regulated,
   logic-heavy domains — healthcare and finance first — who need fast, local,
   offline-capable desktop apps with clearly gated paid features.

## What it is trying to do

Make it possible to repeatedly ship trustworthy desktop SaaS apps from one
foundation, so each new product starts at the domain layer instead of rebuilding
auth, billing, entitlements, licensing, and sync.

## What it must not feel like

- A web app bolted onto a desktop shell — these are local-first, deep-workflow apps.
- A kit where security is optional or where the client can self-report paid access.
- A starter that forces re-litigating infrastructure decisions per product.

## Strategic principles

- **Authority is server-side.** The client never becomes billing or permission authority.
- **Correct path is the easy path.** Architectural decisions are pre-made; agents add domains, not infrastructure.
- **Regulated domains are first-class.** Audit events, sensitive-data classification, and controlled exports are required, not optional.
- **Behavior over shape.** Success is measured by passing behavioral/boundary tests, not code structure.
- **No secrets on the client.** Stripe/DB/webhook/signing secrets never ship in the Tauri app.

## Accessibility

Target accessible-by-default UI for the built products. Record concrete
requirements here as product UIs are designed.
