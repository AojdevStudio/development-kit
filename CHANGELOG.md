# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-06-15

### Added

- Product module plug-in seam (trait + product feature keys + conventions) (Closes #36, #54)
- Feature gate end-to-end for one paid feature (Closes #30, #53)
- Stripe webhook ingestion with idempotent reconcile (#32, #52)
- audit/event service: AuditEvent DTO + cloud recorder (#35, #42)
- feature-key coverage gate harness (#25, #43)
- short-lived token sign/verify + POST /license/refresh (#28, #48)
- leak scan in the security gate (#24, #47)
- auth/account resolve and GET /me (#27, #44)
- cargo-deny dependency bans for the desktop crate (#23, #46)
- offline sync queue with retry, dedup, and conflict policy (#45)
- SQLite migrations, DraftRepository, persistence tests (#40)
- API DTOs, typed plan/status enums, license_expires_at (Closes #21, #41)
- extensible scoped gate check registry (#22, #39)
- walking skeleton: workspace, capability crates, Tauri shell, API health (#20)
- docs: restructure docs and add agent operating config (#6)
- chore(config): add Stripe MCP server, Cursor plugin, and .gitignore
- chore: add agent and CodeGraph project config
- chore: organize docs and add agent skill symlinks
- development kit planning docs

### Changed

- executable Postgres product-migration runner (Closes #64, #68)
- live platform-chrome slot for single-product mode (Closes #66, #67)
- single-product (primary-product) mode so a sole-product app owns the root surface (#65)
- Audit hardening: registry derivation backstop, edges rule-coverage, desktop snapshot fetch (#59, #61)
- Authority hardening: license auth, coverage truthfulness, webhook HMAC (#60)
- Sample product module: Notes (proves the intake contract) (Closes #37, #55)
- Billing checkout and portal sessions (mock mode) (Closes #31, #51)
- Entitlement engine and GET /me/entitlements (#29, #50)

## Links

[Unreleased]: https://github.com/AojdevStudio/development-kit/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/AojdevStudio/development-kit/releases/tag/v0.2.0
