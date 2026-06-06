# Product Module Plug-In Seam

How a product plugs into the kit across all six dimensions (**workflows,
screens, local/cloud tables, Tauri commands, feature keys, tests**) without
editing the shared foundation. This is the contract issue #37 (the sample
product) and every future product follows mechanically.

Read alongside `docs/PRD-INTAKE-CONTRACT.md` (the shape of a product PRD),
`docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md` (the authority model), and the ADRs
(`docs/adr/0001-platform-authority-split.md`,
`docs/adr/0002-capability-crates-and-product-seam.md`).

## The one rule

> A product **extends** the spine by composition; it never **modifies** shared
> foundation code. No edit to `crates/shared` enums, the entitlement engine, the
> gate runner's logic, or the existing route/migration chains.

Everything below is how each dimension honors that rule.

## The namespace is the spine of a product

A product picks one `snake_case` **namespace** (e.g. `vault`). That single value
scopes every dimension, so all six derive from one place
(`shared::ProductModuleMeta`):

| Derives | Convention | Example |
| --- | --- | --- |
| Backend routes | `/<namespace>/…` | `/vault/share` |
| Feature keys | `<namespace>.<name>` | `vault.share_record` |
| Local + cloud tables | `<namespace>_<entity>` | `vault_records` |
| Migration versions | `<namespace>_NNNN_<desc>` | `vault_0001_init` |

`ProductModuleMeta::new(id, namespace)` validates the namespace is `snake_case`
and gives you `route_prefix()` (`/vault`) and `table_prefix()` (`vault_`).

## The two-trait shape (and why)

A product spans two authority sides (ADR-0001/0002): the **cloud backend** owns
routes and authority decisions; the **desktop** owns local SQLite state. Those
sides must not pull each other's dependencies (the desktop never links axum,
Stripe, or sqlx). So the seam is two complementary traits, one per side, sharing
only the namespace identity in the dependency-thin `shared` crate:

| Trait | Crate | Contributes |
| --- | --- | --- |
| `BackendModule` | `services/api` (`product_module.rs`) | `meta()` + an `axum::Router` of product routes |
| `LocalModule` | `crates/local-store` (`product_module.rs`) | `meta()` + the product's `&[Migration]` |
| `ProductModuleMeta` | `crates/shared` (`product_module.rs`) | shared identity: `id` + `namespace` |

`api::product_module::mount(&module)` nests the backend router under
`/<namespace>`; `local_store::product_module::apply_module(conn, &module)` runs
the baseline migrations then the product's. Both are **additive**: they touch no
existing route or migration.

---

## The six dimensions

### 1. Workflows → PRD intake §3 (Product Workflows)

A product's workflows come from PRD §3, each tagged `Local-only`,
`Cloud-backed`, `Hybrid`, or `Billing/account`. They are expressed as:

- **Cloud-backed / billing steps** → routes on the `BackendModule` router,
  guarded by `require_product_feature` where the step is paid.
- **Local-only steps** → Tauri commands (dimension 4) over `LocalModule` tables
  (dimension 3).
- **Hybrid steps** → a local command that reads/writes local state and calls a
  backend route for the authority decision.

Each workflow's `Feature gate:` field (PRD §3) maps to a feature key (dimension
5). The workflow's `Offline behavior:` maps to the sync-queue policy in
`shared::sync_queue`.

### 2. Screens → PRD intake §12 (UI Surface)

A product's React screens come from PRD §12. Convention:

- Screens live under `apps/desktop/src/products/<namespace>/` (e.g.
  `apps/desktop/src/products/vault/RecordsScreen.tsx`).
- Routes are registered in the desktop shell's router under a `/<namespace>`
  path segment, mirroring the backend prefix.
- Each screen's `Feature gate:` column (PRD §12) drives a **UX-only** React
  guard. Per ADR-0001 this is presentation only. The authoritative gate is the
  Tauri command and/or backend route (dimensions 4 + 5). Never gate a paid screen
  in React alone.

### 3. Local & cloud tables → PRD intake §6 (Local SQLite) and §7 (Cloud Postgres)

- **Local SQLite** (PRD §6): the product's `LocalModule::migrations()` returns
  `Migration`s creating `<namespace>_<entity>` tables. They append to the
  baseline schema via `apply_module`; the shared `schema_migrations` ledger keeps
  them idempotent. Local tables are authoritative for local product state only,
  **never** for subscription/billing state (ADR-0001).
- **Cloud Postgres** (PRD §7): the product's Postgres tables follow the same
  `<namespace>_<entity>` prefix under `migrations/postgres/`. (The executable
  Postgres product-migration runner is wired in a later issue; the convention
  (namespaced prefix, authority state only on the cloud) is fixed here.)

### 4. Tauri commands → PRD intake §3/§8 (enforcement layer = "Tauri Rust command")

- Commands live in `apps/desktop/src-tauri/src/products/<namespace>/` and are
  registered in the desktop `invoke_handler!` macro in
  `apps/desktop/src-tauri/src/lib.rs` (one line per command, additive).
- A command guarding a paid local action calls the local feature-gate decision
  (the `decide_feature` pattern in `apps/desktop/src-tauri/src/feature_gate.rs`,
  generalized to product keys) over the entitlement snapshot the desktop fetched
  from the backend. The command, not the screen, is the local authority.

### 5. Feature keys → PRD intake §8 (Feature Tiers and Entitlements)

**The hard part, and the deliberate design decision.** `shared::FeatureKey` is a
**closed enum**, and `Entitlements.features` is keyed on it. A product cannot add
a variant without editing the foundation, which the seam forbids. Resolution:

- A product declares each gated capability as a `shared::ProductFeatureKey`: a
  validated `namespace.name` string key, in a key-space **provably disjoint** from
  the baseline enum. Baseline keys are dotless `snake_case` (`export_pdf`);
  product keys always carry exactly one `.` (`vault.share_record`). The `.`
  separator is the collision-proof invariant: the two key-spaces can never
  overlap on the wire, so a product key can never inherit a baseline grant and a
  baseline key can never be mistaken for a product key.
- The backend resolves an account's product access into a
  `shared::ProductEntitlements` snapshot, parallel to `Entitlements`, keyed on
  `ProductFeatureKey`, with the **identical** `allows` semantics (boolean true /
  non-zero limit / absent-denies). So a product gate asks the same authority
  question the spine asks.
- The backend gate is `api::feature_gate::require_product_feature(&snapshot,
  &key)` → 403/Ok, mirroring the baseline `require_feature`.
- **The coverage gate counts product keys.** A product registers each key in
  `xtask::coverage::product_key_registry()` and records its non-React (Tauri
  command or backend) gate test in `product_coverage_manifest()`. The gate
  (`cargo xtask gate --scope billing|security`) runs the baseline *and* product
  halves: a product paid key with no command/backend gate test fails CI exactly
  as a baseline key would. React-only enforcement is structurally impossible:
  `GateLayer` has no React variant.

Why not just add an `Other(String)` variant to `FeatureKey`? Because that edits
the foundation, breaks the exhaustive `plan_features` match and `FeatureKey::ALL`
length guard, and lets the closed-enum discipline rot one product at a time. A
disjoint parallel key-space keeps the spine closed and the product space open,
the open/closed principle applied to the feature vocabulary. (Recorded in
`docs/adr/0002-capability-crates-and-product-seam.md`.)

**What this seam proves, and what the sample product (#37) added.**
The seam proves the *type and enforcement shape*: a product key validates,
serializes, gates (`require_product_feature`), and is counted by the coverage
gate. The seam's in-repo test double (`vault`, in `tests/product_module.rs`)
reads the snapshot from the request *body* for simplicity. That is a TEST
SCAFFOLD, not the authority pattern.

The **sample product (#37, `notes`) closed that loop**: its paid route
`POST /notes/publish` resolves `ProductEntitlements` SERVER-SIDE from the caller's
bearer token (auth, then account state, then the product's per-tier policy
`resolve_product_entitlements`), exactly the way the authenticated spine gate
(`/gated-feature/{feature}`) resolves baseline entitlements. The request body is
never read for the authority decision, so a lying/forged body can never grant a
paid product key (`services/api/tests/notes_product.rs` pins this). A new product
copies the `notes` route shape, not the `vault` body-reading scaffold. Where a
product has not yet wired server-side resolution, treat product gating as
deny-by-default (an empty snapshot grants nothing), which is the safe direction.

Also note: `ProductEntitlements` is a **parallel** path that *mirrors* the
baseline enforcement question, not the literal same `BTreeMap<FeatureKey, …>`
path. The closed enum makes a literal-same path impossible, which is the whole
reason the seam exists. The guarantee is "the same authority question, asked the
same way," not "the same data structure."

### 6. Tests → PRD intake §14 (Acceptance Tests)

A product's tests come from PRD §14 and live with the code they exercise:

- **Shared types** → `#[cfg(test)]` in the product's `shared`-side modules (if
  any product types land there) or in the product crate.
- **Backend routes / gates** → `services/api/tests/<namespace>_*.rs`, driven
  through the real router via `tower::ServiceExt::oneshot` (no socket).
- **Local store** → `crates/local-store/tests/<namespace>_*.rs` against an
  in-memory DB.
- **Tauri commands** → `#[cfg(test)]` in the command module.
- **Feature-gate coverage** → the named test in `product_coverage_manifest()`
  must be a real `#[test]`/`#[tokio::test]`; `cargo test` runs it in the same
  gate, so coverage can never be claimed without a passing enforcement test.

Tests target external behavior and authority boundaries, never private internals
(Goal PRD "Testing Decisions").

---

## Worked example (the in-repo test double)

The seam ships with a tiny `vault` example exercised by the test suite (NOT the
full sample product, #37):

- `services/api/tests/product_module.rs`: a `VaultModule: BackendModule` with a
  `/vault/ping` route and a `/vault/share` route gated on `vault.share_record`;
  proves additive composition, namespace prefixing, two-product non-collision,
  and product-key gating through the real `api::app()`.
- `crates/local-store/src/product_module.rs` tests: a `VaultLocal: LocalModule`
  with a `vault_records` table; proves migrations apply additively, idempotently,
  and the table is queryable.
- `xtask::coverage`: registers `vault.share_record` and records its backend
  gate test, so the live coverage run counts it.

A product author copies this shape: implement the two traits, declare keys,
register coverage, add tests, and the module plugs in.

## Checklist for a new product module

- [ ] Pick a `snake_case` namespace; build `ProductModuleMeta`.
- [ ] Implement `BackendModule` (routes) and `LocalModule` (migrations).
- [ ] Mount via `mount(&module)` / `apply_module(conn, &module)` (additive).
- [ ] Declare each paid capability as a `ProductFeatureKey`.
- [ ] Resolve product access into `ProductEntitlements`; gate with
      `require_product_feature` (backend) and the command-layer guard (desktop).
- [ ] Register every key in `product_key_registry()` and its non-React gate test
      in `product_coverage_manifest()`.
- [ ] Tables named `<namespace>_<entity>`; no billing authority local.
- [ ] Screens under `products/<namespace>/`; React gate is UX only.
- [ ] Tests per PRD §14, by behavior and boundary.
- [ ] `cargo xtask gate --scope all` green; `bun` frontend green.
