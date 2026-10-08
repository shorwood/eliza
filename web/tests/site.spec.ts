import { execFileSync } from 'node:child_process'
import { expect, test } from '@playwright/test'

test('provider requests are copyable and browsing never calls generation', async ({ page, context }) => {
  const errors: string[] = []
  const generation: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  page.on('request', request => {
    if (/\/(openai|anthropic|gemini|ollama)\//.test(request.url())) generation.push(request.url())
  })
  await page.goto('/')
  await page.keyboard.press('Tab')
  await expect(page.getByRole('link', { name: 'Skip to content' })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page.locator('#main')).toBeFocused()
  await expect(page.getByRole('heading', { name: 'A test endpoint for your AI client.' })).toBeVisible()
  await expect(page.locator('a[href="/support"]')).toHaveCount(0)
  for (const [index, path] of ['/openai/v1/chat/completions', '/anthropic/v1/messages', '/gemini/v1beta/models/eliza-1966:generateContent', '/ollama/api/chat'].entries()) {
    await page.getByLabel('Provider', { exact: true }).selectOption(String(index))
    const command = await page.getByLabel('Example curl request').innerText()
    expect(command).toContain(path)
    execFileSync('bash', ['-n'], { input: command })
  }
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  await page.getByRole('button', { name: 'Copy request' }).click()
  await expect(page.getByRole('status')).toHaveText('Command copied.')
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('/ollama/api/chat')
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width)
  expect(generation).toEqual([])
  expect(errors).toEqual([])
})

test('copy failures explain manual recovery', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) } })
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Copy request' }).click()
  await expect(page.getByRole('status')).toHaveText('Select and copy the command below.')
})

test('documentation and sample request work without JavaScript', async ({ browser, baseURL }, testInfo) => {
  const context = await browser.newContext({ javaScriptEnabled: false, viewport: testInfo.project.name === 'phone' ? { width: 393, height: 851 } : { width: 1280, height: 800 } })
  const page = await context.newPage()
  await page.goto(baseURL!)
  await expect(page.getByLabel('Example curl request')).toContainText('/openai/v1/chat/completions')
  await page.getByRole('link', { name: 'Read the documentation' }).click()
  await page.getByRole('navigation', { name: 'Documentation', exact: true }).getByRole('link', { name: 'CI with GitHub Actions or Jenkins' }).click()
  await expect(page.getByRole('heading', { name: 'CI with GitHub Actions or Jenkins' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Jenkins', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width)
  await page.getByRole('link', { name: 'View Markdown' }).click()
  expect(page.url()).toMatch(/\/docs\/ci\.md$/)
  await context.close()
})

test('hydrated documentation stays inside the configured viewport', async ({ page }) => {
  await page.goto('/docs/ci')
  await expect(page.getByRole('heading', { name: 'Jenkins', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width)
})

test('support and account remain private and cannot fake activation', async ({ page, request }) => {
  await page.goto('/support?checkout=success&session_id=untrusted')
  await expect(page.getByRole('heading', { name: 'Support ELIZA.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Checkout unavailable' })).toBeDisabled()
  await expect(page.getByText('This page cannot take a payment.', { exact: false })).toBeVisible()
  await page.goto('/account?token=untrusted&error=expired')
  await expect(page.getByRole('button', { name: 'Sign-in unavailable' })).toBeDisabled()
  await expect(page.locator('input')).toHaveCount(0)
  for (const path of ['/support', '/account']) {
    const response = await request.get(path)
    expect(response.headers()['cache-control']).toContain('no-store')
    expect(await response.text()).toContain('noindex')
  }
})

test('public assets exclude SDK installations and builds', async ({ request }) => {
  expect((await request.get('/docs/quickstart.md')).status()).toBe(200)
  const schema = await (await request.get('/openapi.json')).json()
  expect(schema.paths['/openai/v1/chat/completions']).toBeTruthy()
  expect((await request.get('/examples/rust-rig/Cargo.toml')).status()).toBe(200)
  for (const path of ['/examples/vercel-ai/node_modules/ai/package.json', '/examples/rust-rig/target/debug/eliza-public-rig-example', '/docs/missing-page']) {
    expect((await request.get(path)).status()).toBe(404)
  }
  const sitemap = await (await request.get('/sitemap.xml')).text()
  expect(sitemap).toContain('/docs/quickstart')
  expect(sitemap).not.toContain('/support')
  expect(sitemap).not.toContain('/account')
  expect(await (await request.get('/robots.txt')).text()).toContain('Disallow: /account')
})
