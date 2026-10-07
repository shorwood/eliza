BEGIN;
SELECT pg_advisory_xact_lock(16016);
CREATE TABLE IF NOT EXISTS accounts (
  id uuid PRIMARY KEY,
  email text NOT NULL UNIQUE,
  customer_id text UNIQUE,
  checkout_id text,
  reconciled_at timestamptz NOT NULL DEFAULT '-infinity',
  next_reconcile_at timestamptz NOT NULL DEFAULT '-infinity',
  reconcile_failures integer NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS credentials (
  hash text PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  kind text NOT NULL CHECK (kind IN ('login', 'session')),
  expires_at timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS credentials_expiry ON credentials(expires_at);
CREATE TABLE IF NOT EXISTS subscriptions (
  id text PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  status text NOT NULL,
  paid_until timestamptz NOT NULL,
  grace_until timestamptz NOT NULL
);
CREATE TABLE IF NOT EXISTS api_keys (
  id uuid PRIMARY KEY,
  account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  hash text NOT NULL UNIQUE,
  label text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  expires_at timestamptz,
  revoked_at timestamptz
);
CREATE INDEX IF NOT EXISTS keys_account ON api_keys(account_id);
CREATE TABLE IF NOT EXISTS billing_events (
  id text PRIMARY KEY,
  customer_id text NOT NULL,
  attempts integer NOT NULL DEFAULT 0,
  retry_at timestamptz NOT NULL DEFAULT now(),
  done_at timestamptz
);
CREATE TABLE IF NOT EXISTS rate_limits (
  hash text PRIMARY KEY,
  count integer NOT NULL,
  expires_at timestamptz NOT NULL
);
COMMIT;
