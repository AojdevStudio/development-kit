# Security Policy

`development-kit` is a pre-1.0, agent-buildable starter kit for desktop SaaS apps (Tauri v2 + React/Vite + Rust + local SQLite on the client; a cloud Rust/Axum backend + Postgres + Stripe Billing as the authority). Its defining property is a hard client/cloud authority split, enforced as a compile fact via mechanical `cargo xtask gate` checks: the desktop physically cannot issue licenses, reach Postgres, or hold billing secrets. Security reports against that authority boundary are the ones we care most about.

This policy covers **only the code in this repository** (`AojdevStudio/development-kit`). It does **not** cover any product, deployment, or fork built *from* the kit. See [If you build a product on this kit](#if-you-build-a-product-on-this-kit).

## Reporting a Vulnerability

**Do not open a public GitHub issue, pull request, or discussion for a security vulnerability.** Public disclosure before a fix is available puts every downstream fork at risk. Report privately first.

**Preferred channel (GitHub Private Vulnerability Reporting, PVR):**

1. Go to the repository's **Security** tab → **Advisories**. (The Security tab may be tucked under a **`...`** dropdown on the repo nav.)
2. Click **Report a vulnerability**.
3. Fill in the advisory form (title and description are required) and **Submit report**.

This opens a private advisory visible only to you and the maintainer. If you want to collaborate on a fix, you can start a temporary private fork from the advisory.

If you do **not** see a **Report a vulnerability** button, Private Vulnerability Reporting may not be enabled yet. Use the fallback email channel below.

**Fallback channel (email):**

If PVR is unavailable to you, email the maintainer at **`admin@unifiedental.com`**. Use a clear subject line such as `SECURITY: development-kit`. We do not currently publish a PGP key, so please do not include live secrets, customer data, or other sensitive material in plaintext email. Describe the issue and provide a minimal proof-of-concept instead.

### What to include in a report

A good report is much faster to triage and act on. Please include as much of the following as you can:

- **Affected version / commit:** the `main` commit SHA you tested against (there are no release tags yet).
- **Component / file:** e.g. `services/api/src/webhook.rs`, `crates/license-sign/`, `xtask/src/edges.rs`.
- **Impact:** what authority boundary breaks and what an attacker gains (e.g. "forge a paid license token", "apply a Stripe event twice", "leak the signing key into the desktop tree").
- **Reproduction steps:** exact commands; ideally a failing test or a `cargo xtask gate` slice that should be red but is green.
- **Proof-of-concept:** minimal code, payload, or token that demonstrates the boundary actually failing.
- **Environment:** OS, Rust toolchain, Bun version, and any non-default configuration.

Because this kit is built on **codified gates over honesty rules**, the most actionable reports show one of those gates or boundaries actually failing, not a theoretical concern.

## Supported Versions

This is a pre-1.0 project (workspace version `0.1.0`). It is consumed by **forking the repository** and building product features into the spine; there is no distributed released binary, published package, or release tag yet.

| Version / branch | Supported |
|---|---|
| `main` (latest commit) | Yes: fixes land here |
| A future `0.x` tag, once one is cut | Best-effort, most recent line only |
| Older snapshots | No backports |
| Forks / products built from the kit | No (see below) |

Notes:

- **There are no release tags yet.** Track `main` and cite the **commit SHA** you tested against. Once a line is tagged, only the most recent tag receives fixes.
- SemVer is declared, but the project is in `0.x`: API and authority surfaces can still change between minor versions.
- Security fixes land on `main`. There is **no coordinated multi-version patch release and no back-porting** to older snapshots.
- If you have forked the kit, **you are responsible for pulling fixes forward** into your fork.

## Response & Disclosure Expectations

This is a pre-1.0, open-source project maintained on a **best-effort basis by a single, AI-assisted maintainer**. There is no dedicated security team and no SLA.

- We **aim** to acknowledge new reports **within a few business days**, but cannot guarantee a fixed timeframe: a single maintainer's availability varies.
- After acknowledgement we aim to provide an initial assessment, then prioritize by severity and available time.
- We **cannot guarantee** fixed response, triage, or remediation timelines.

These are goals stated in good faith, consistent with GitHub's coordinated-disclosure guidance to acknowledge receipt as quickly as possible even when no immediate resources are available.

### Coordinated disclosure

- Please report privately and give the maintainer a **reasonable opportunity to remediate before any public disclosure**. We'll agree on a disclosure timeline together in the advisory, scaled to severity and the realities of a single-maintainer project. Please don't anchor to a fixed industry default.
- When an issue is confirmed, we **intend** to disclose it via **GitHub Security Advisories** on this repository, and may request a CVE where warranted. That is where consumers should watch for fix announcements.
- We will **credit reporters** who follow this policy, unless you prefer to remain anonymous.

## Scope

### In scope

Reports that demonstrate a break in the kit's **authority boundary** are the highest value:

- **License token forgery or replay:** minting valid tokens without backend authority; `crates/license-verify` accepting forged, tampered, or expired tokens; mishandling of the ed25519 signing key in `crates/license-sign`.
- **Server-side entitlement bypass / privilege escalation:** granting paid features without a valid entitlement (`services/api/src/entitlement.rs`), cross-account access, trusting client-reported plan/features, or a body-supplied `account_id` in `POST /license/refresh` (`services/api/src/license.rs`).
- **Stripe webhook signature bypass:** flaws in `StripeWebhookVerifier` HMAC verification, constant-time comparison, or timestamp tolerance, **or** idempotency bypass (double-applied events) in `services/api/src/webhook.rs`.
- **Secret / key leakage into the desktop:** Stripe secret/restricted/webhook keys, `DATABASE_URL`, the ed25519 signing key, or PEM private keys reaching desktop **source** or the built **artifact**, including bypasses of the leak scan (`xtask/src/leakscan.rs`) or the `cargo-deny` / crate-edge authority enforcement.
- **Authentication flaws** in `services/api/src/auth.rs`: bearer-token resolution, impersonation, cross-account principal resolution, or incorrect `MissingCredentials` vs `InvalidToken` handling.
- **Injection:** SQL injection or other injection in the Axum API / Postgres query layer.
- **Sync / offline tampering:** treating local SQLite as billing authority, offline license-token expiry/revocation bypass, or sync-queue / conflict-policy manipulation that escalates access.
- **React-only feature gating:** a paid capability reachable with no non-React (Tauri-command or backend) gate (the hole the coverage gate in `xtask/src/coverage.rs` and `seam_scan.rs` exists to close).
- **Breaks in the mechanical authority enforcement itself:** a forbidden crate edge, an unconstrained new workspace package escaping the default-deny edge rules (`xtask/src/edges.rs`), or any `cargo xtask gate` check that can be silently bypassed.

### Out of scope

- Vulnerabilities in a downstream **fork's** own product code, product feature keys, product modules, or product-specific business logic. Those belong to the fork, not the kit spine.
- **Theoretical findings with no proof-of-concept** against the authority boundary. Show the gate or boundary actually failing.
- Vulnerabilities in pinned third-party dependencies already surfaced by Dependabot / `cargo-deny` advisories, **unless** the kit's own usage turns a non-issue into an exploitable authority bypass. Report dependency issues upstream.
- **Social engineering**, phishing of the maintainer, or compromise of a contributor's GitHub / Stripe / cloud account.
- **Denial-of-service** from a self-hoster's own misconfiguration: e.g. an unhardened self-hosted CI runner (explicitly not provisioned today), resource exhaustion of a self-run API instance, or missing rate limits a product is expected to add.
- Findings that depend on a **product or operator failing to layer their own security**: HIPAA/PCI controls, a real auth provider, TLS, or production secret management. The kit ships mock / in-memory dev stores and a mock webhook verifier **by design**.
- **Missing production hardening that is explicitly deferred** to the consuming product (durable Postgres-backed stores, rate limiting, a real auth backend). That is documented future work, not a defect in the shipped authority model.
- Issues reachable only by running the kit **outside its documented architecture**: e.g. pointing the desktop directly at Postgres, or shipping secrets that `.env.example` explicitly warns against committing.

## Safe Harbor

We support good-faith security research and will not pursue action against researchers who follow this policy.

- We will consider your security research to be **authorized** if you make a good-faith effort to comply with this policy during your research.
- For **inadvertent, good-faith violations** of this policy, we will not take civil action or file a report with law enforcement.
- If a third party takes legal action against you for activities carried out in accordance with this policy, we will make this authorization known to the extent we are reasonably able.
- Before doing anything that may be inconsistent with, or unaddressed by, this policy, please contact us first by submitting a report.

**Limits.** This authorization:

- is conditioned on your **good-faith compliance with this policy** and staying within scope;
- covers **only the code in this repository** (not third-party services, dependencies, or any production deployment built from the kit);
- does **not** waive the rights of any third party and cannot authorize anything illegal or anything the maintainer lacks the authority to permit;
- does **not** extend to data destruction, privacy violations, denial-of-service, or accessing accounts or data you do not own.

Safe harbor authorizes **research**; it does not warrant the software. This project is provided "AS IS", without warranty of any kind, per its open-source license. Nothing here is a representation or certification that the software is secure or defect-free.

## If You Build a Product on This Kit

**This kit is the trusted platform *spine*, not a finished, compliant product.** Its gates prove the **authority model** is correct: the server decides entitlements, the client can only verify, and no secrets ship to the client. They do **not** prove that any product built on it is HIPAA- or PCI-compliant, and this project claims **no formal certification** (SOC 2, HIPAA, PCI, ISO 27001) for itself.

By design the kit ships **dev-grade scaffolding**: in-memory `PrincipalStore` / `AccountStateStore` / `AuditStore`, a `MockWebhookVerifier` and mock billing sessions, a dev-seeded Pro account, no real auth provider, no durable Postgres-backed stores yet, and no TLS / rate-limiting / secret-manager wiring.

If you fork this kit (**especially for healthcare or finance**, which the kit treats as first-class targets), **you own the security posture of what you ship**. At minimum you must establish your own:

- **`SECURITY.md` and disclosure process** for your product. This policy covers only the upstream starter-kit code.
- **Real authentication and session management**, replacing the in-memory dev stores.
- **Durable, access-controlled audit storage with retention:** the audit DTO carries a `Sensitivity` enum; billing / permission / security actions must be tagged `Sensitive` and always persisted (never sampled out), with the durable authority living in cloud Postgres.
- **Sensitive-data classification and encryption** for your own domain data, plus **controlled-export** rules.
- **Compliance posture:** BAAs, PCI scope decisions, and any required certification (none of which the kit provides).
- **Rate limiting and abuse controls**, production secret management, TLS, and your own threat model and acceptance tests.

The PRD intake contract makes this explicit: a product PRD missing roles, feature tiers, data classification, offline behavior, sync rules, or acceptance tests is a stop-and-escalate condition, and security-sensitive entitlement / license / webhook / migration changes are flagged for human review. Downstream vulnerabilities in a fork's own product code are the fork's responsibility, not the kit's.

---

*This policy applies to the `main` branch of `AojdevStudio/development-kit` (and any future tagged line), and lives at one of GitHub's recognized paths (`SECURITY.md` in the repo root, `/docs`, or `/.github`) so it is auto-linked from the Security tab. It may change as the project matures toward 1.0.*
