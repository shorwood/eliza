import { createHash, randomBytes, randomUUID } from 'node:crypto'
import type { Sql, TransactionSql } from 'postgres'

export const DAY = 86_400_000
export const hash = (value: string) => createHash('sha256').update(value).digest('hex')
const secret = () => randomBytes(32).toString('base64url')

export class BillingError extends Error {
  constructor(public status: number, message: string) { super(message) }
}

export interface Snapshot {
  id: string
  status: string
  paidUntil: number
}

export interface Payments {
  customer(accountId: string, email: string): Promise<string>
  subscriptions(customer: string): Promise<Snapshot[]>
  checkout(customer: string, previous: string | null): Promise<{ id: string, url: string }>
  portal(customer: string): Promise<string>
}

type Account = { id: string, email: string, customer_id: string | null, checkout_id: string | null }

// All state lives in PostgreSQL. Connections and clients may be reused by Nitro.
export class Billing {
  constructor(
    readonly sql: Sql,
    readonly payments: Payments,
    readonly sendLogin: (email: string, token: string) => Promise<void>,
    readonly now = () => Date.now(),
  ) {}

  async limit(scope: string, identity: string, maximum: number, seconds: number) {
    const start = Math.floor(this.now() / (seconds * 1000)) * seconds * 1000
    const key = hash(`${scope}:${identity}:${start}`)
    const [row] = await this.sql`
      INSERT INTO rate_limits (hash, count, expires_at) VALUES (${key}, 1, ${new Date(start + seconds * 1000)})
      ON CONFLICT (hash) DO UPDATE SET count = rate_limits.count + 1 RETURNING count
    `
    if (row!.count > maximum) throw new BillingError(429, 'Please try again later.')
  }

  async login(email: string) {
    const normalized = email.trim().toLowerCase()
    if (normalized.length > 254 || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(normalized)) {
      throw new BillingError(400, 'Enter a valid email address.')
    }
    await this.limit('email-login', normalized, 3, 900)
    const [account] = await this.sql<Account[]>`
      INSERT INTO accounts (id, email) VALUES (${randomUUID()}, ${normalized})
      ON CONFLICT (email) DO UPDATE SET email = excluded.email RETURNING *
    `
    const token = secret()
    await this.sql`INSERT INTO credentials (hash, account_id, kind, expires_at)
      VALUES (${hash(token)}, ${account!.id}, 'login', ${new Date(this.now() + 15 * 60_000)})`
    try { await this.sendLogin(normalized, token) }
    catch {
      await this.sql`DELETE FROM credentials WHERE hash = ${hash(token)}`
      throw new BillingError(503, 'Sign-in email could not be sent. Please try again.')
    }
  }

  async consume(token: string) {
    const session = secret()
    await this.sql.begin(async (sql) => {
      const [row] = await sql`DELETE FROM credentials WHERE hash = ${hash(token)} AND kind = 'login'
        AND expires_at > ${new Date(this.now())} RETURNING account_id`
      if (!row) throw new BillingError(401, 'This sign-in link has expired or was already used.')
      await sql`INSERT INTO credentials (hash, account_id, kind, expires_at)
        VALUES (${hash(session)}, ${row.account_id}, 'session', ${new Date(this.now() + 7 * DAY)})`
    })
    return session
  }

  async account(session: string): Promise<Account> {
    const [account] = await this.sql<Account[]>`SELECT a.* FROM accounts a JOIN credentials c ON c.account_id = a.id
      WHERE c.hash = ${hash(session)} AND c.kind = 'session' AND c.expires_at > ${new Date(this.now())}`
    if (!account) throw new BillingError(401, 'Please sign in.')
    return account
  }

  async logout(session: string) {
    await this.sql`DELETE FROM credentials WHERE hash = ${hash(session)} AND kind = 'session'`
  }

  async summary(accountId: string) {
    const subscriptions = await this.sql`SELECT id, status, paid_until, grace_until FROM subscriptions
      WHERE account_id = ${accountId}`
    const keys = await this.sql`SELECT id, label, created_at, expires_at, revoked_at FROM api_keys
      WHERE account_id = ${accountId} ORDER BY created_at DESC`
    const until = Math.max(0, ...subscriptions.map(s => Math.max(+s.paid_until, +s.grace_until)))
    return { eligible: until > this.now(), eligibleUntil: until || null, subscriptions, keys }
  }

  // Lock BEFORE fetching Stripe: concurrent/reordered deliveries cannot commit an older fetch last.
  async reconcile(customerId: string) {
    await this.sql.begin(sql => this.reconcileWith(sql, customerId))
  }

  private async reconcileWith(sql: TransactionSql, customerId: string) {
    const [account] = await sql<Account[]>`SELECT * FROM accounts WHERE customer_id = ${customerId} FOR UPDATE`
    if (!account) return
    const snapshots = await this.payments.subscriptions(customerId)
    for (const snapshot of snapshots) {
      const [old] = await sql`SELECT paid_until FROM subscriptions WHERE id = ${snapshot.id}`
      const paidUntil = Math.max(snapshot.paidUntil, old ? +old.paid_until : 0)
      const graceUntil = ['past_due', 'unpaid'].includes(snapshot.status) && paidUntil > 0 ? paidUntil + 7 * DAY : 0
      await sql`INSERT INTO subscriptions (id, account_id, status, paid_until, grace_until)
        VALUES (${snapshot.id}, ${account.id}, ${snapshot.status}, ${new Date(paidUntil)}, ${new Date(graceUntil)})
        ON CONFLICT (id) DO UPDATE SET status = excluded.status, paid_until = excluded.paid_until,
          grace_until = excluded.grace_until`
    }
    // A removed subscription may keep its paid period, but never its renewal-failure grace.
    const ids = snapshots.map(s => s.id)
    if (ids.length) await sql`UPDATE subscriptions SET status = 'canceled', grace_until = ${new Date(0)}
      WHERE account_id = ${account.id} AND id NOT IN ${sql(ids)}`
    else await sql`UPDATE subscriptions SET status = 'canceled', grace_until = ${new Date(0)} WHERE account_id = ${account.id}`
    await sql`UPDATE accounts SET reconciled_at = ${new Date(this.now())}, reconcile_failures = 0,
      next_reconcile_at = ${new Date(this.now() + 60_000)} WHERE id = ${account.id}`
  }

  async checkout(accountId: string) {
    return this.sql.begin(async (sql) => {
      const [account] = await sql<Account[]>`SELECT * FROM accounts WHERE id = ${accountId} FOR UPDATE`
      if (!account) throw new BillingError(401, 'Please sign in.')
      if (!account.customer_id) {
        account.customer_id = await this.payments.customer(account.id, account.email)
        await sql`UPDATE accounts SET customer_id = ${account.customer_id} WHERE id = ${account.id}`
      }
      const subscriptions = await this.payments.subscriptions(account.customer_id)
      if (subscriptions.some(s => !['canceled', 'incomplete_expired'].includes(s.status))) {
        throw new BillingError(409, 'A subscription already exists. Use billing management instead.')
      }
      const checkout = await this.payments.checkout(account.customer_id, account.checkout_id)
      await sql`UPDATE accounts SET checkout_id = ${checkout.id} WHERE id = ${account.id}`
      return checkout.url
    })
  }

  async portal(accountId: string) {
    const [account] = await this.sql<Account[]>`SELECT * FROM accounts WHERE id = ${accountId}`
    if (!account?.customer_id) throw new BillingError(409, 'No billing account exists yet.')
    return this.payments.portal(account.customer_id)
  }

  async createKey(accountId: string, label: string, rotateId?: string) {
    if (!label.trim() || label.length > 80) throw new BillingError(400, 'Use a key label of 1–80 characters.')
    const id = randomUUID()
    const value = `eliza_test_${id}_${secret()}`
    await this.sql.begin(async (sql) => {
      await sql`SELECT id FROM accounts WHERE id = ${accountId} FOR UPDATE`
      const [eligibility] = await sql`SELECT 1 FROM subscriptions WHERE account_id = ${accountId}
        AND greatest(paid_until, grace_until) > ${new Date(this.now())} LIMIT 1`
      if (!eligibility) throw new BillingError(403, 'A confirmed subscription is required to create keys.')
      if (rotateId) {
        const [old] = await sql`UPDATE api_keys SET expires_at = least(coalesce(expires_at, 'infinity'), ${new Date(this.now() + DAY)})
          WHERE id = ${rotateId} AND account_id = ${accountId} AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > ${new Date(this.now())}) RETURNING id`
        if (!old) throw new BillingError(404, 'Key not found.')
      }
      const [count] = await sql`SELECT count(*)::int AS total FROM api_keys WHERE account_id = ${accountId}
        AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ${new Date(this.now())})`
      if (count!.total >= 20) throw new BillingError(409, 'Revoke an unused key before creating another.')
      await sql`INSERT INTO api_keys (id, account_id, hash, label) VALUES (${id}, ${accountId}, ${hash(value)}, ${label.trim()})`
    })
    return { id, value }
  }

  async revoke(accountId: string, id: string) {
    const [key] = await this.sql`UPDATE api_keys SET revoked_at = coalesce(revoked_at, ${new Date(this.now())})
      WHERE id = ${id} AND account_id = ${accountId} RETURNING id`
    if (!key) throw new BillingError(404, 'Key not found.')
  }

  // Only a restricted Rust-to-Nitro read path calls this; it never contacts Stripe.
  async entitlement(keyHash: string) {
    const [row] = await this.sql`SELECT k.account_id, k.revoked_at, k.expires_at,
      max(greatest(s.paid_until, s.grace_until)) AS eligible_until FROM api_keys k
      LEFT JOIN subscriptions s ON s.account_id = k.account_id WHERE k.hash = ${keyHash}
      GROUP BY k.id`
    const until = row ? Math.min(+row.eligible_until || 0, row.expires_at ? +row.expires_at : Infinity) : 0
    return { eligible: !!row && !row.revoked_at && until > this.now(),
      accountId: row?.account_id ?? null, eligibleUntil: until || null, refreshSeconds: 60, staleSeconds: 300 }
  }

  async enqueue(id: string, customer: string) {
    await this.sql`INSERT INTO billing_events (id, customer_id, retry_at)
      VALUES (${id}, ${customer}, ${new Date(this.now())}) ON CONFLICT DO NOTHING`
  }

  // A crash rolls the transaction back; the pending row remains claimable next pass.
  async processEvent(id: string) {
    try {
      await this.sql.begin(async (sql) => {
        const [event] = await sql`SELECT * FROM billing_events WHERE id = ${id} AND done_at IS NULL
          AND retry_at <= ${new Date(this.now())} FOR UPDATE SKIP LOCKED`
        if (!event) return
        await this.reconcileWith(sql, event.customer_id)
        await sql`UPDATE billing_events SET done_at = ${new Date(this.now())} WHERE id = ${id}`
      })
    }
    catch {
      await this.sql`UPDATE billing_events SET attempts = attempts + 1,
        retry_at = ${new Date(this.now())} + least(3600, 30 * power(2, least(attempts, 7))) * interval '1 second'
        WHERE id = ${id} AND done_at IS NULL`
      // No Stripe payload, email, token or provider exception is logged.
    }
  }

  async repair() {
    const pending = await this.sql`SELECT id FROM billing_events WHERE done_at IS NULL
      AND retry_at <= ${new Date(this.now())} ORDER BY retry_at LIMIT 25`
    for (const event of pending) await this.processEvent(event.id)
    const accounts = await this.sql`SELECT customer_id FROM accounts WHERE customer_id IS NOT NULL
      AND next_reconcile_at <= ${new Date(this.now())} ORDER BY next_reconcile_at LIMIT 25`
    let failures = 0
    for (const account of accounts) {
      try { await this.reconcile(account.customer_id) }
      catch {
        failures++
        await this.sql`UPDATE accounts SET reconcile_failures = reconcile_failures + 1,
          next_reconcile_at = ${new Date(this.now())} + least(3600, 60 * power(2, least(reconcile_failures, 6))) * interval '1 second'
          WHERE customer_id = ${account.customer_id}`
      }
    }
    await this.sql`DELETE FROM credentials WHERE expires_at <= ${new Date(this.now())}`
    await this.sql`DELETE FROM rate_limits WHERE expires_at <= ${new Date(this.now())}`
    await this.sql`DELETE FROM billing_events WHERE done_at < ${new Date(this.now() - 30 * DAY)}`
    return { checked: accounts.length, failures, pending: pending.length }
  }
}
