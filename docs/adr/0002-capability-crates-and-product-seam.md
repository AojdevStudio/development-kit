# ADR-0002: Capability crates and the product-module seam

- **Status:** Accepted
- **Date:** 2026-06-05
- **Deciders:** AojdevStudio

## Context

ADR-0001 fixes the platform authority split (desktop vs cloud). Two further
forces need a recorded decision:

1. **Compile-time enforcement of the split.** A policy that "the desktop never
   issues licenses, reaches Postgres, or touches Stripe" is only as strong as its
   weakest reviewer unless it is a *compile fact*. The kit already encodes this as
   **capability crates** plus a mechanical crate-edge gate; this ADR records that
   decision so the many `(ADR-0002)` references in the code and gate have a home.

2. **How a product plugs in without editing the foundation (issue #36).** The kit
   must accept many products on one spine. The platform vocabulary is closed for
   safety: `shared::FeatureKey` is a closed enum and the feature-key coverage gate
   iterates it. A product cannot add a gated capability by widening that enum
   without editing shared foundation code, which would let the closed-enum
   discipline rot one product at a time and break the exhaustive entitlement
   policy match and the `FeatureKey::ALL` coverage guard.

## Decision

### Capability crates (compile-time authority enforcement)

Authority capabilities are isolated in dedicated crates, and a package's
dependency graph is the contract:

- `license-sign` (issuance, holds the private key) is a dependency of
  `services/api` only. `license-verify` (verification) is a dependency of the
  desktop only. The desktop tree does not list `license-sign`, `sqlx`,
  `tokio-postgres`, or Stripe crates; `services/api` does not list
  `license-verify`.
- `crates/shared` stays types-only: no `sqlx`, no Stripe, no crypto, no secret
  loaders. It is the one crate both authority sides may depend on, which is what
  lets them share types without dragging authority-bearing dependencies across
  the boundary.
- `cargo xtask gate` runs an **ADR-0002 crate-edge check** that fails the moment a
  forbidden edge appears, plus a leak scan over the desktop source and built
  artifact. The capability is absent by construction; the gate is the backstop.

### Product-module seam (issue #36)

A product **extends** the spine by composition; it never **modifies** shared
foundation code. The seam is two complementary traits sharing one identity:

- `api::product_module::BackendModule`: contributes an `axum::Router`, mounted
  additively under `/<namespace>` by `mount`.
- `local_store::product_module::LocalModule`: contributes SQLite `Migration`s,
  applied additively after the baseline schema by `apply_module`.
- `shared::ProductModuleMeta`: the shared identity (`id` + `namespace`) both sides
  derive routes, tables, and feature keys from. It lives in `shared` so neither
  authority side depends on the other.

**Feature keys (the load-bearing decision).** A product declares gated
capabilities as `shared::ProductFeatureKey`, a validated `namespace.name` string
key in a key-space provably disjoint from the closed `FeatureKey` enum: baseline
keys are dotless `snake_case`, product keys always carry exactly one `.`. The
backend resolves product access into a `ProductEntitlements` snapshot with
identical `allows` semantics to `Entitlements`, gated by
`require_product_feature`. The feature-key coverage gate counts product keys
through a `product_key_registry()` + `product_coverage_manifest()`, holding them
to the same non-React-gate standard as baseline keys. Full conventions:
`docs/PRODUCT-MODULE-SEAM.md`.

## Consequences

- **Positive:** the authority split is a compile fact, not a convention; products
  start at the domain layer; the platform feature vocabulary stays closed (safe,
  exhaustive) while the product vocabulary is open (extensible). React-only
  enforcement of a product paid key is structurally impossible.
- **Negative / trade-offs:** product keys are stringly-typed (validated at the
  boundary) rather than a single exhaustive enum, so a product owns the
  discipline of registering its keys for coverage. Two parallel entitlement
  shapes (`Entitlements`, `ProductEntitlements`) exist instead of one.
- **Follow-ups:** issue #37 (sample product) follows `docs/PRODUCT-MODULE-SEAM.md`
  mechanically; the executable cloud Postgres product-migration runner is a later
  issue (the namespaced-prefix convention is fixed here).

## Alternatives considered

- **Widen `FeatureKey` with an `Other(String)` variant.** Rejected: edits the
  foundation per product, breaks the exhaustive `plan_features` match and the
  `FeatureKey::ALL` coverage guard, and erodes the closed-enum safety the spine
  relies on.
- **A runtime feature-key registry replacing the enum entirely.** Rejected: the
  baseline keys benefit from exhaustiveness and compile-time checking; only the
  *product* space needs to be open. A disjoint parallel key-space keeps both
  properties.
- **One combined `ProductModule` trait naming both `axum::Router` and
  `Migration`.** Rejected: it would force one authority side to depend on the
  other's crate, violating the capability-crate boundary above.
