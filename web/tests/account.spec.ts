import { randomBytes, randomUUID } from 'node:crypto'
import { request as httpRequest } from 'node:http'
import postgres from 'postgres'
import { expect, test } from '@playwright/test'
import { hash } from '../server/billing/service'
import { testStripe } from '../server/billing/stripe'

if (!process.env.TEST_DATABASE_URL || !new URL(process.env.TEST_DATABASE_URL).pathname.endsWith('_test')) {
  throw new Error('TEST_DATABASE_URL must point to a disposable database ending in _test. Run test:billing first.')
}
let sql: ReturnType<typeof postgres>
test.beforeAll(() => { sql = postgres(process.env.TEST_DATABASE_URL!, { max: 2 }) })
const origin = 'http://127.0.0.1:18880'
test.afterAll(async () => { await sql.end() })

async function seed() {
  const id = randomUUID()
  const token = randomBytes(32).toString('base64url')
  await sql`INSERT INTO accounts (id, email, customer_id) VALUES (${id}, ${`${id}@example.test`}, ${`cus_${id}`})`
  await sql`INSERT INTO credentials (hash, account_id, kind, expires_at)
    VALUES (${hash(token)}, ${id}, 'login', ${new Date(Date.now() + 900_000)})`
  await sql`INSERT INTO subscriptions (id, account_id, status, paid_until, grace_until)
    VALUES (${`sub_${id}`}, ${id}, 'active', ${new Date(Date.now() + 86400_000)}, ${new Date(0)})`
  return { id, token }
}

test('email link needs explicit confirmation; keys reveal once, rotate and revoke', async ({ page }, testInfo) => {
  const { id, token } = await seed()
  await page.goto(`${origin}/account#login=${token}`)
  await expect(page.getByRole('button', { name: 'Confirm sign-in' })).toBeVisible()
  expect(page.url()).toBe(`${origin}/account`)
  expect(await sql`SELECT * FROM credentials WHERE hash = ${hash(token)}`).toHaveLength(1)
  await page.getByRole('button', { name: 'Confirm sign-in' }).click()
  await expect(page.getByRole('button', { name: 'Create key' })).toBeVisible()
  // Returning from hosted Stripe is a cross-site top-level GET. Preserve the session.
  await page.goto('http://localhost:18878/')
  await page.goto(`${origin}/account`)
  await expect(page.getByRole('button', { name: 'Create key' })).toBeVisible()
  await page.getByRole('button', { name: 'Create key' }).click()
  await expect(page.getByRole('heading', { name: 'Save this key now' })).toBeVisible()
  const key = await page.locator('.secret-key').innerText()
  const [stored] = await sql`SELECT * FROM api_keys WHERE account_id = ${id}`
  expect(stored!.hash).toBe(hash(key))
  await page.reload()
  await expect(page.locator('.secret-key')).toHaveCount(0)
  await page.getByRole('button', { name: 'Rotate CI', exact: true }).click()
  await expect(page.getByRole('status')).toContainText('24 hours')
  await expect(page.locator('.key-list li')).toHaveCount(2)
  await page.locator('.key-list li').first().getByRole('button', { name: 'Revoke CI', exact: true }).click()
  await expect(page.locator('.key-list li').first()).toContainText('Revoked')
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width)
  await page.screenshot({ path: `tools/screenshots/account-${testInfo.project.name}.png`, fullPage: true })
  await page.getByRole('button', { name: 'Sign out' }).click()
  await expect(page.getByRole('button', { name: 'Email a sign-in link' })).toBeVisible()
})

test('HTTP sessions reject foreign origins and access to another account key', async ({ request }) => {
  const owner = await seed()
  expect((await request.post(`${origin}/account/confirm`, { data: { token: owner.token }, headers: { Origin: 'https://evil.example' } })).status()).toBe(403)
  const confirm = await request.post(`${origin}/account/confirm`, { data: { token: owner.token }, headers: { Origin: origin } })
  expect(confirm.status()).toBe(200)
  const cookie = confirm.headers()['set-cookie']!
  expect(cookie).toContain('HttpOnly')
  expect(cookie).toContain('SameSite=Lax')
  expect((await request.post(`${origin}/account/confirm`, { data: { token: owner.token }, headers: { Origin: origin } })).status()).toBe(401)
  const other = await seed()
  const id = randomUUID()
  await sql`INSERT INTO api_keys (id, account_id, hash, label) VALUES (${id}, ${other.id}, ${hash(id)}, 'Other CI')`
  expect((await request.post(`${origin}/account/keys/revoke`, { data: { id }, headers: { Origin: origin } })).status()).toBe(404)
  expect((await request.post(`${origin}/account/keys/rotate`, { data: { id, label: 'stolen' }, headers: { Origin: origin } })).status()).toBe(404)
  const state = await (await request.get(`${origin}/account/state`)).json()
  expect(state.account.email).toBe(`${owner.id}@example.test`)
  expect(state.account.keys).toHaveLength(0)
})

test('raw signed webhook persists before ack; live or tampered notifications rejected', async ({ request }) => {
  const owner = await seed()
  const stripe = testStripe('sk_test_fixture')
  const id = `evt_${randomUUID()}`
  const payload = JSON.stringify({ id, type: 'invoice.paid', livemode: false, data: { object: { customer: `cus_${owner.id}` } } })
  const signature = stripe.webhooks.generateTestHeaderString({ payload, secret: 'whsec_local_fixture' })
  const options = { data: payload, headers: { 'Stripe-Signature': signature, 'Content-Type': 'application/json' } }
  expect((await request.post(`${origin}/billing/webhook`, options)).status()).toBe(200)
  expect((await request.post(`${origin}/billing/webhook`, options)).status()).toBe(200)
  expect(await sql`SELECT * FROM billing_events WHERE id = ${id}`).toHaveLength(1)
  expect((await request.post(`${origin}/billing/webhook`, { ...options, data: `${payload} ` })).status()).toBe(400)
  const live = payload.replace('"livemode":false', '"livemode":true')
  const liveSignature = stripe.webhooks.generateTestHeaderString({ payload: live, secret: 'whsec_local_fixture' })
  expect((await request.post(`${origin}/billing/webhook`, { data: live, headers: { 'Stripe-Signature': liveSignature } })).status()).toBe(400)
})

test('entitlement endpoint is restricted, no-store and does not call Stripe', async ({ request }) => {
  const owner = await seed()
  const key = randomBytes(32).toString('hex')
  const id = randomUUID()
  await sql`INSERT INTO api_keys (id, account_id, hash, label) VALUES (${id}, ${owner.id}, ${hash(key)}, 'CI')`
  expect((await request.post(`${origin}/billing/entitlement`, { data: hash(key) })).status()).toBe(401)
  const options = { data: hash(key), headers: { Authorization: 'Bearer local-entitlement-fixture-32-bytes-only' } }
  const response = await request.post(`${origin}/billing/entitlement`, options)
  expect(response.status()).toBe(200)
  expect(response.headers()['cache-control']).toContain('no-store')
  expect(await response.json()).toMatchObject({ eligible: true, refreshSeconds: 60, staleSeconds: 300 })
  await sql`UPDATE api_keys SET revoked_at = now() WHERE id = ${id}`
  expect(await (await request.post(`${origin}/billing/entitlement`, options)).json()).toMatchObject({ eligible: false })
  expect(await (await request.post(`${origin}/billing/entitlement`, { ...options, data: hash('unknown') })).json()).toMatchObject({ eligible: false })
})

test('account and billing reject oversized uploads before the client finishes', async () => {
  for (const [path, maximum, headers] of [
    ['/account/login', 4096, { Origin: origin, 'Content-Type': 'application/json' }],
    ['/billing/entitlement', 64, { Authorization: 'Bearer local-entitlement-fixture-32-bytes-only' }],
    ['/billing/webhook', 1_048_576, { 'Stripe-Signature': 'invalid' }],
  ] as const) {
    for (const declared of [true, false]) {
      const status = await new Promise<number>((resolve, reject) => {
        const request = httpRequest(`${origin}${path}`, { method: 'POST',
          headers: { ...headers, ...(declared ? { 'Content-Length': String(maximum + 1) } : {}) },
          signal: AbortSignal.timeout(3000) }, (response) => {
          response.resume()
          response.on('end', () => { resolve(response.statusCode!); request.destroy() })
        })
        request.on('error', reject)
        // Deliberately omit end(): both declared and chunked limits must reject before EOF.
        if (declared) request.flushHeaders()
        else request.write(Buffer.alloc(maximum + 1, 32))
      })
      expect(status).toBe(413)
    }
  }
})
