# Project Context Docs

How engineering and design skills consume this repo's project context before
exploring the codebase or changing UI. This repo uses a **single-context**
layout.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — domain language and the platform authority model.
- **`docs/adr/`** — read ADRs (`*.md`) that touch the area you're about to work in.
- **`PRODUCT.md`** at the repo root — read before UI, UX, product, copy, onboarding, or design-system work.
- **`DESIGN.md`** at the repo root — read before visual UI work.

`PRODUCT.md` and `DESIGN.md` are resolved case-insensitively using the
`impeccable` convention:

1. `IMPECCABLE_CONTEXT_DIR`, if set
2. repo root
3. `.agents/context/`
4. `docs/`

## Architecture source documents

This kit has authoritative design docs the context files summarize. Treat these
as canonical when they conflict with generic assumptions:

- `docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md` — the absolute responsibility split (Tauri vs cloud backend vs Postgres vs Stripe).
- `docs/PRD-INTAKE-CONTRACT.md` — how a product PRD plugs into the platform spine.
- `docs/DEVELOPMENT-KIT-GOAL-PRD.md` — the goal, acceptance criteria, and verification surfaces.

## File structure

Single-context repo:

```text
/
├── CONTEXT.md
├── PRODUCT.md
├── DESIGN.md
├── docs/adr/
│   └── 0001-platform-authority-split.md
└── docs/
    ├── TAURI-STRIPE-SAAS-ARCHITECTURE.md
    ├── PRD-INTAKE-CONTRACT.md
    └── DEVELOPMENT-KIT-GOAL-PRD.md
```

## Use the glossary's vocabulary

When your output names a domain concept (issue title, refactor proposal,
hypothesis, test name), use the term as defined in `CONTEXT.md`. Don't drift to
synonyms. If a concept isn't in the glossary yet, that's a signal — either
you're inventing language the project doesn't use (reconsider) or there's a real
gap (note it for `/grill-with-docs`).

## Use product and design vocabulary for UI work

When your output names a user, workflow, surface, brand attribute, design
principle, color, type role, component, or visual state, use the term as defined
in `PRODUCT.md` or `DESIGN.md`. `PRODUCT.md` is strategic; `DESIGN.md` is visual
and operational. Do not duplicate visual rules into `PRODUCT.md`.

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than
silently overriding:

> _Contradicts ADR-0001 (platform authority split) — but worth reopening because…_
