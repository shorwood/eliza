import { readFile } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'
import postgres from 'postgres'
import { afterAll, beforeAll, beforeEach, expect, test, vi } from 'vitest'
import { Billing, DAY, hash, type Payments, type Snapshot } from '../../server/billing/service'
import { testStripe } from '../../server/billing/stripe'
import { loginMailer } from '../../server/billing/mail'

if (!process.env.TEST_DATABASE_URL || !new URL(process.env.TEST_DATABASE_URL).pathname.endsWith('_test')) {
  throw new Error('TEST_DATABASE_URL must point to a disposable database ending in _test.')
}
const sql = postgres(process.env.TEST_DATABASE_URL, { max: 4, onnotice: () => {} })
let now = Date.UTC(2026, 9, 7)
let snapshots: Snapshot[] = []
const mail = vi.fn<(email: string, token: string) => Promise<void>>()
const payments: Payments = {
  customer: vi.fn(async () => `cus_${randomUUID()}`),
  subscriptions: vi.fn(async () => snapshots),
  checkout: vi.fn(async () => ({ id: 'cs_test', url: 'https://checkout.stripe.com/test' })),
  portal: vi.fn(async customer => `https://billing.stripe.com/${customer}`),
}
const billing = () => new Billing(sql, payments, mail, () => now)
beforeAll(async () => {
  const migration = postgres(process.env.TEST_DATABASE_URL!, { max: 1, onnotice: () => {} })
  try { await migration.unsafe(await readFile(new URL('../../server/billing/schema.sql', import.meta.url), 'utf8')) }
  finally { await migration.end() }
})
beforeEach(async () => {
  await sql`TRUNCATE accounts, credentials, subscriptions, api_keys, billing_events, rate_limits CASCADE`
  now = Date.UTC(2026, 9, 7)
  snapshots = []
  vi.clearAllMocks()
  mail.mockResolvedValue(undefined)
  vi.mocked(payments.subscriptions).mockReset().mockImplementation(async () => snapshots)
})
afterAll(async () => { await sql.end() })

async function account(email = 'builder@example.test') {
  await billing().login(email)
  const token = mail.mock.lastCall![1]
  const session = await billing().consume(token)
  return { session, ...await billing().account(session) }
}
async function paid() {
  const owner = await account()
  await billing().checkout(owner.id)
  const [row] = await sql`SELECT customer_id FROM accounts WHERE id = ${owner.id}`
  const customer = row!.customer_id as string
  snapshots = [{ id: 'sub_test', status: 'active', paidUntil: now + 30 * DAY }]
  await billing().reconcile(customer)
  return { ...owner, customer }
}

test('single-use login, expiry, session revocation and hash-only credentials', async () => {
  await billing().login(' BUILDER@EXAMPLE.TEST ')
  const token = mail.mock.lastCall![1]
  const [stored] = await sql`SELECT * FROM credentials`
  expect(stored!.hash).toBe(hash(token))
  const results = await Promise.allSettled([billing().consume(token), billing().consume(token)])
  expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(1)
  const result = results.find(r => r.status === 'fulfilled')!
  if (result.status !== 'fulfilled') throw new Error()
  expect((await billing().account(result.value)).email).toBe('builder@example.test')
  await billing().logout(result.value)
  await expect(billing().account(result.value)).rejects.toMatchObject({ status: 401 })
  await billing().login('builder@example.test')
  const expired = mail.mock.lastCall![1]
  now += 16 * 60_000
  await expect(billing().consume(expired)).rejects.toMatchObject({ status: 401 })
})

test('mail failure removes its login token; login and checkout are durably limited', async () => {
  mail.mockRejectedValueOnce(new Error('secret provider error'))
  await expect(billing().login('builder@example.test')).rejects.toMatchObject({ status: 503 })
  expect(await sql`SELECT * FROM credentials`).toHaveLength(0)
  await billing().limit('test', 'a', 1, 900)
  await expect(billing().limit('test', 'a', 1, 900)).rejects.toMatchObject({ status: 429 })
  now += 900_000
  await expect(billing().limit('test', 'a', 1, 900)).resolves.toBeUndefined()
})

test('checkout and active status alone cannot grant benefits or create keys', async () => {
  const owner = await account()
  await billing().checkout(owner.id)
  expect((await billing().summary(owner.id)).eligible).toBe(false)
  snapshots = [{ id: 'sub_test', status: 'active', paidUntil: 0 }]
  const [row] = await sql`SELECT customer_id FROM accounts WHERE id = ${owner.id}`
  await billing().reconcile(row!.customer_id)
  await expect(billing().createKey(owner.id, 'CI')).rejects.toMatchObject({ status: 403 })
  await expect(billing().checkout(owner.id)).rejects.toMatchObject({ status: 409 })
})

test('payment, renewal, failure, fixed seven-day grace and paid-through cancellation', async () => {
  const owner = await paid()
  const paidThrough = now + 30 * DAY
  expect((await billing().summary(owner.id)).eligibleUntil).toBe(paidThrough)
  snapshots = [{ id: 'sub_test', status: 'past_due', paidUntil: 0 }]
  now = paidThrough + 6 * DAY
  await billing().reconcile(owner.customer)
  expect((await billing().summary(owner.id)).eligibleUntil).toBe(paidThrough + 7 * DAY)
  now += 2 * DAY
  await billing().reconcile(owner.customer)
  expect((await billing().summary(owner.id)).eligible).toBe(false)
  snapshots = [{ id: 'sub_test', status: 'active', paidUntil: now + 30 * DAY }]
  await billing().reconcile(owner.customer)
  const renewed = now + 30 * DAY
  snapshots = [{ id: 'sub_test', status: 'canceled', paidUntil: 0 }]
  await billing().reconcile(owner.customer)
  expect((await billing().summary(owner.id)).eligibleUntil).toBe(renewed)
  now = renewed + 1
  expect((await billing().summary(owner.id)).eligible).toBe(false)
})

test('rotation overlap, owner isolation and late events never resurrect revoked keys', async () => {
  const owner = await paid()
  const other = await account('other@example.test')
  const key = await billing().createKey(owner.id, 'CI')
  const rotated = await billing().createKey(owner.id, 'CI rotated', key.id)
  expect((await billing().entitlement(hash(key.value))).eligible).toBe(true)
  now += DAY + 1
  expect((await billing().entitlement(hash(key.value))).eligible).toBe(false)
  expect((await billing().entitlement(hash(rotated.value))).eligible).toBe(true)
  await expect(billing().revoke(other.id, rotated.id)).rejects.toMatchObject({ status: 404 })
  await expect(billing().createKey(other.id, 'stolen', rotated.id)).rejects.toMatchObject({ status: 403 })
  await billing().revoke(owner.id, rotated.id)
  await billing().enqueue('evt_late', owner.customer)
  await billing().processEvent('evt_late')
  expect((await billing().entitlement(hash(rotated.value))).eligible).toBe(false)
  expect((await billing().entitlement(hash('unknown'))).eligible).toBe(false)
  const stored = await sql`SELECT hash FROM api_keys`
  expect(stored.map(k => k.hash)).toContain(hash(rotated.value))
  expect(JSON.stringify(await billing().summary(owner.id))).not.toContain(rotated.value)
  expect(payments.portal).not.toHaveBeenCalled()
  const calls = vi.mocked(payments.subscriptions).mock.calls.length
  await billing().entitlement(hash(rotated.value))
  expect(payments.subscriptions).toHaveBeenCalledTimes(calls)
})

test('durable duplicate/reordered events reconcile current state after restart', async () => {
  const owner = await paid()
  await Promise.all([billing().enqueue('evt_new', owner.customer), billing().enqueue('evt_new', owner.customer)])
  await billing().enqueue('evt_old', owner.customer)
  snapshots = [{ id: 'sub_test', status: 'canceled', paidUntil: 0 }]
  // A fresh service instance models a lost Vercel invocation after durable insert.
  await billing().processEvent('evt_new')
  await billing().processEvent('evt_old')
  await billing().processEvent('evt_new')
  expect(await sql`SELECT * FROM billing_events`).toHaveLength(2)
  expect((await sql`SELECT * FROM billing_events WHERE done_at IS NULL`)).toHaveLength(0)
  expect((await billing().summary(owner.id)).subscriptions[0]!.status).toBe('canceled')
})

test('failed processing stays durable, retries with backoff, and missed events are repaired', async () => {
  const owner = await paid()
  await billing().enqueue('evt_retry', owner.customer)
  vi.mocked(payments.subscriptions).mockRejectedValueOnce(new Error('provider unavailable'))
  await billing().processEvent('evt_retry')
  const [failed] = await sql`SELECT * FROM billing_events`
  expect(failed!.attempts).toBe(1)
  expect(failed!.done_at).toBeNull()
  await billing().processEvent('evt_retry')
  expect((await sql`SELECT * FROM billing_events`)[0]!.attempts).toBe(1)
  now += 61_000
  snapshots = [{ id: 'sub_test', status: 'active', paidUntil: now + 60 * DAY }]
  await billing().repair()
  expect((await sql`SELECT * FROM billing_events`)[0]!.done_at).not.toBeNull()
  now += 61_000
  snapshots = [{ id: 'sub_test', status: 'past_due', paidUntil: 0 }]
  await billing().repair()
  expect((await billing().summary(owner.id)).subscriptions[0]!.status).toBe('past_due')
})

test('concurrent event processing does not exhaust the connection pool or overwrite newer state', async () => {
  const owner = await paid()
  let calls = 0
  vi.mocked(payments.subscriptions).mockImplementation(async () => {
    calls++
    await new Promise(resolve => setTimeout(resolve, 10))
    return [{ id: 'sub_test', status: calls === 1 ? 'active' : 'canceled', paidUntil: 0 }]
  })
  await Promise.all(Array.from({ length: 8 }, async (_, i) => {
    const id = `evt_${i}`
    await billing().enqueue(id, owner.customer)
    await billing().processEvent(id)
  }))
  expect((await billing().summary(owner.id)).subscriptions[0]!.status).toBe('canceled')
  expect(await sql`SELECT * FROM billing_events WHERE done_at IS NULL`).toHaveLength(0)
})

test('live credentials rejected and raw webhook signature tampering rejected', () => {
  expect(() => testStripe('sk_live_fake')).toThrow('Only Stripe test mode')
  const stripe = testStripe('sk_test_fake')
  const payload = JSON.stringify({ id: 'evt_test', livemode: false })
  const signature = stripe.webhooks.generateTestHeaderString({ payload, secret: 'whsec_test' })
  expect(stripe.webhooks.constructEvent(payload, signature, 'whsec_test').id).toBe('evt_test')
  expect(() => stripe.webhooks.constructEvent(`${payload} `, signature, 'whsec_test')).toThrow()
})

test('SMTP requires verified TLS remotely and cannot enable secret logging through URL options', () => {
  expect(() => loginMailer('smtp://mail.example.test/?debug=true')).toThrow('without query overrides')
  expect(() => loginMailer('https://mail.example.test')).toThrow()
  const mail = loginMailer('smtp://username:password@mail.example.test:587')
  expect(mail.options).toMatchObject({ requireTLS: true, tls: { rejectUnauthorized: true }, debug: false })
  mail.close()
})

test('failed account reconciliation backs off without starving other accounts', async () => {
  const owner = await paid()
  const other = await account('other@example.test')
  snapshots = []
  await billing().checkout(other.id)
  now += 61_000
  vi.mocked(payments.subscriptions).mockImplementation(async (customer) => {
    if (customer === owner.customer) throw new Error('unavailable')
    return [{ id: 'sub_other', status: 'active', paidUntil: now + 30 * DAY }]
  })
  expect(await billing().repair()).toMatchObject({ checked: 2, failures: 1 })
  expect((await billing().summary(other.id)).eligible).toBe(true)
  const calls = vi.mocked(payments.subscriptions).mock.calls.length
  now++
  expect(await billing().repair()).toMatchObject({ checked: 0, failures: 0 })
  expect(payments.subscriptions).toHaveBeenCalledTimes(calls)
})
