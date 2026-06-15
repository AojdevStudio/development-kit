# ADR-0003: Mechanical authority-boundary enforcement

- **Status:** Accepted
- **Date:** 2026-06-05
- **Deciders:** AojdevStudio (with Obi, `grill-with-docs` session)
- **Supersedes:** none. Extends ADR-0001 (platform authority split), taking it from *documented* to *enforced*. Sibling to ADR-0002 (capability crates and the product-module seam): ADR-0002 records the crate structure and owns the `(ADR-0002)`-tagged code references; this ADR details the full enforcement mechanism (gate scopes, CI, branch protection, feature-key coverage).

## Context

ADR-0001 declares the authority split non-negotiable, but states it only as prose.
An agent that ignores the boundary hits no wall. It can add `sqlx` to the desktop
crate, ship a Stripe secret, or gate a paid feature in React alone, and nothing
fails. For a billing/entitlement kit reused across regulated-domain products, that
is the exact failure mode that bites in production (a charge, a sync, or an offline
edge case) long after a plausible-looking review passed.

Decision principle: **codified gates over honesty rules.** An enforced gate is
proof; a behavioral instruction is hope. The boundary must be mechanical and
enforced *outside* the agent.

Runtime context: this kit is built via Claude Code (OAuth subscription), not the
Anthropic API. Model routing therefore means which model/agent the session or
dynamic workflow runs per issue (see `docs/agents/triage-labels.md`), not API
model selection.

## Decision

Enforce the ADR-0001 authority split through layered, **compile-time-first**
mechanisms, all runnable from one entrypoint and enforced by CI.

1. **Substrate: compile-time-first, layered.** Express violations as build
   failures wherever the type system and crate graph can; cover the residue with
   CI scans and behavioral tests.

2. **Crate graph: fine-grained capability crates.**
   - `crates/shared`: pure types only (feature-key enums, entitlement/DTO models). Zero heavy deps (no `sqlx`, no Stripe, no secret loaders).
   - `crates/license-verify`: public-key **verify** only. Desktop-safe.
   - `crates/license-sign`: private-key **sign** only. Backend-only; never in the desktop tree.
   - `apps/desktop` depends on `shared` + `license-verify` only.
   - `services/api` depends on `shared` + `license-sign` + `sqlx` + Stripe.

   "Desktop can only verify, never issue" and "no DB/Stripe on the client" become
   compile facts. `cargo-deny` bans are the CI backstop, not the primary defense.
   License scheme: **ed25519** (assumption; finalize in the license issue).

3. **Gate runner: `cargo xtask gate`.** One Rust binary orchestrates every check
   (`cargo fmt`, `clippy -D warnings`, workspace tests, `cargo-deny`, source plus
   built-artifact secret-scan, Bun lint/type/test/build, SQLite + Postgres
   migrations, Stripe webhook fixtures, feature-key coverage), shelling to Bun for
   the frontend slice. `--scope desktop|api|frontend|db|billing|security|prd|all`
   runs per-issue slices. Called identically by local dev, CI, and the dynamic
   workflow. Cross-platform (Tauri ships Win/macOS/Linux), zero extra install.

4. **Enforcement teeth: GitHub Actions + required check on `main`.** CI runs
   `cargo xtask gate` on every PR; branch protection makes a green gate a required
   status check. Merge is physically blocked until green, non-bypassable by agent
   or human. CI runs check/clippy/test/build plus the debug-artifact secret-scan,
   not full multi-platform installer bundling.

5. **React-never-sole-gate: feature-key coverage gate.** Every `FeatureKey`
   variant in `crates/shared` must have at least one passing Tauri-command and/or
   backend gate test (per its declared enforcement layer). A paid key with no
   non-React gate test fails CI. Scales to every product plugged in via the intake
   contract.

6. **Runner backup: runner-agnostic, flip on demand.** `runs-on` is driven by a
   repo variable (default free GitHub-hosted standard runner); the `gate` check
   name is fixed so branch protection is never touched on a runner swap. A hardened
   self-hosted recipe is documented and ready: ephemeral, isolated
   (network-segmented, no homelab access), secret-free, and approval-gated for
   outside collaborators. The repo is **public**, so a naive self-hosted runner is
   a fork-PR RCE foothold. Public standard minutes are currently free, so no live
   self-hosted runner is provisioned today.

### Pattern to layer map

| Authority rule | Mechanism | Layer |
|---|---|---|
| Desktop cannot reach Postgres/`sqlx` | crate graph (no dep) + `cargo-deny` ban | compile |
| Desktop cannot reach Stripe SDK | crate graph + `cargo-deny` ban | compile |
| Desktop cannot issue/sign licenses | `license-sign` absent from desktop tree | compile |
| No secrets in desktop (`sk_*`, `whsec_`, `postgres://`, PEM keys) | source-scan + built-artifact scan | CI scan |
| Webhook idempotency | replay-same-event-id fixture yields single state change | test |
| Backend is entitlement authority | entitlement-calc + deny-without-entitlement tests | test |
| License verify / expiry / tamper | sign-in-backend, verify-in-desktop tests | test |
| Local SQLite is not billing authority | entitlements read from backend/license-token, not a writable local table | test + convention |
| React never sole gate | feature-key coverage gate | CI coverage |

## Consequences

- **Positive:** authority violations fail at build/CI, not review; "done" is
  externally verified, not self-reported; the guard scales to every product;
  the gate is portable across runners.
- **Negative / trade-offs:** roughly 4 small crates instead of 2; an `xtask`
  harness to build and maintain; every feature key must ship with a gate test;
  CI minutes (free on public standard runners today).
- **Follow-ups (become the platform-spine Epic's first issues):** scaffold
  `crates/{shared,license-verify,license-sign}`; `cargo-deny` config; the `xtask
  gate` harness; GitHub Actions workflow + branch protection; source/artifact
  secret-scan; feature-key coverage harness; the documented self-hosted runner
  recipe.

## Alternatives considered

- **CI static checks first (no crate-graph bans).** Rejected: violations still
  compile and only fail at PR time; the check scripts live in files the drifting
  agent can edit.
- **Test-suite-only assertions.** Rejected: the same agent that drifts can skip
  or delete a test; weakest guarantee for billing/entitlement code.
- **Coarse crates + `cargo-deny` ban-list.** Rejected as primary: no physical
  separation, so a forbidden dependency nobody added to the ban-list compiles fine.
- **Honesty rules / reviewer rubric.** Rejected: replaced by codified gates per
  the build doctrine.
- **Active self-hosted runner now.** Rejected: public standard minutes are free,
  so a live runner is maintenance plus RCE attack surface for no current benefit.
