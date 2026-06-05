-- Postgres bootstrap migration (cloud authority state).
--
-- Walking skeleton: establishes the migration tracking table only. The durable
-- SaaS authority schema (users, accounts, subscriptions, entitlements, license
-- metadata, webhook events, audit) lands in its own issue, mapped to the data
-- model in docs/TAURI-STRIPE-SAAS-ARCHITECTURE.md.

CREATE TABLE IF NOT EXISTS schema_migrations (
    version     TEXT PRIMARY KEY,
    applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
