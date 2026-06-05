# DESIGN — Development Kit

Visual system for products built on the kit. Read before visual UI work.

> Seed stub — no visual system is defined yet. Generate a real one with
> `impeccable document` once the desktop app shell exists, or fill these sections
> in manually. Until then, proceed once per session after noting this is a stub.

## Overview

Local-first desktop SaaS UI (Tauri v2 + React + Vite). Prioritize density,
keyboard-driven workflows, offline clarity (clearly show synced vs queued
state), and unambiguous paid-feature gating. Healthcare/finance contexts call
for calm, high-legibility, low-chrome surfaces.

## Colors

_TBD — define theme tokens (light/dark), semantic roles (success/warning/danger),
and an explicit "offline / queued" state color._

## Typography

_TBD — define type scale and roles (display, heading, body, mono for data)._

## Elevation

_TBD — define surface levels and shadow/border conventions._

## Components

_TBD — define core components: gated-feature affordance, sync-status indicator,
entitlement/upgrade prompts, audit-friendly tables._

## Do's and Don'ts

- **Do** make gated paid features visually explicit; never hide them silently.
- **Do** surface offline/queued state so users trust local-first behavior.
- **Don't** imply access the backend hasn't granted (React gating is UX, not security).
- **Don't** duplicate strategic rules from `PRODUCT.md` here.
