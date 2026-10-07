import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { randomBytes, randomUUID } from 'node:crypto'
import postgres from 'postgres'
import { expect, test } from '@playwright/test'
import { hash } from '../server/billing/service'

const database = process.env.TEST_DATABASE_URL
if (!database || !new URL(database).pathname.endsWith('_test')) throw new Error('Use a disposable TEST_DATABASE_URL ending in _test.')
const sql = postgres(database, { max: 2 })
const billing = 'http://127.0.0.1:18880'
const ingress = 'joint-origin-fixture-secret-at-least-32-bytes'
test.afterAll(async () => { await sql.end() })

async function unusedPort() {
  const socket = createServer()
  socket.listen(0, '127.0.0.1')
  await once(socket, 'listening')
  const address = socket.address()
  if (!address || typeof address === 'string') throw new Error('Expected TCP address')
  await new Promise<void>((resolve, reject) => socket.close(error => error ? reject(error) : resolve()))
  return address.port
}

test('real billing activation and warm-key revocation reach the Rust origin', async ({ request }, info) => {
  test.skip(info.project.name !== 'desktop', 'One backend integration run covers both viewports.')
  test.setTimeout(120_000)
  const account = randomUUID()
  const login = randomBytes(32).toString('base64url')
  await sql`INSERT INTO accounts (id, email) VALUES (${account}, ${`${account}@example.test`})`
  await sql`INSERT INTO credentials (hash, account_id, kind, expires_at)
    VALUES (${hash(login)}, ${account}, 'login', ${new Date(Date.now() + 900_000)})`
  await sql`INSERT INTO subscriptions (id, account_id, status, paid_until, grace_until)
    VALUES (${`sub_${account}`}, ${account}, 'active', ${new Date(Date.now() + 86400_000)}, ${new Date(0)})`
  expect((await request.post(`${billing}/account/confirm`, { data: { token: login }, headers: { Origin: billing } })).status()).toBe(200)
  const created = await request.post(`${billing}/account/keys/create`, { data: { label: 'Joint origin CI' }, headers: { Origin: billing } })
  expect(created.status()).toBe(200)
  const key = await created.json() as { id: string, value: string }
  await sql`UPDATE subscriptions SET paid_until = ${new Date(0)}, grace_until = ${new Date(0)} WHERE account_id = ${account}`
  const directory = await mkdtemp(join(tmpdir(), 'eliza-hosted-test-'))
  const config = JSON.parse(await readFile(new URL('../../config/hosted-staging.json', import.meta.url), 'utf8'))
  config.entitlement_url = `${billing}/billing/entitlement`
  const policy = join(directory, 'hosted.json')
  await writeFile(policy, JSON.stringify(config))
  const port = await unusedPort()
  const origin = `http://127.0.0.1:${port}`
  const binary = process.env.ELIZA_TEST_ORIGIN_BINARY ?? resolve('../target/debug/eliza')
  const child = spawn(binary, ['serve', '--bind', `127.0.0.1:${port}`, '--hosted-config', policy], {
    env: { ...process.env, ELIZA_INGRESS_SECRET: ingress, ELIZA_ENTITLEMENT_SECRET: 'local-entitlement-fixture-32-bytes-only' },
    stdio: 'ignore',
  })
  let launchError: Error | undefined
  child.on('error', error => { launchError = error })
  const headers = { 'x-eliza-ingress-token': ingress, 'x-eliza-client-ip': '192.0.2.90' }
  const generate = () => request.post(`${origin}/openai/v1/chat/completions`, {
    headers: { ...headers, Authorization: `Bearer ${key.value}` },
    data: { model: 'eliza-1966', messages: [{ role: 'user', content: 'Hello' }] },
  })
  try {
    await expect.poll(async () => {
      if (launchError) throw launchError
      try { return (await request.get(`${origin}/healthz`, { headers })).status() }
      catch { return 0 }
    }, { timeout: 10_000 }).toBe(200)
    expect((await generate()).status()).toBe(401)
    await sql`UPDATE subscriptions SET status = 'active', paid_until = ${new Date(Date.now() + 86400_000)} WHERE account_id = ${account}`
    await expect.poll(async () => (await generate()).status(), { timeout: 10_000, intervals: [250, 500, 1000] }).toBe(200)
    expect((await request.post(`${billing}/account/keys/revoke`, { data: { id: key.id }, headers: { Origin: billing } })).status()).toBe(200)
    await expect.poll(async () => (await generate()).status(), { timeout: 65_000, intervals: [1000] }).toBe(401)
    const denied = await generate()
    expect(denied.status()).toBe(401)
    expect(await denied.text()).not.toContain('$5')
  }
  finally {
    if (child.exitCode === null && !launchError) {
      const exited = once(child, 'exit')
      child.kill()
      await exited
    }
    await rm(directory, { recursive: true, force: true })
    await sql`DELETE FROM accounts WHERE id = ${account}`
  }
})
