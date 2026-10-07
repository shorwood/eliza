import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testIgnore: '**/billing/**',
  testDir: './tests',
  fullyParallel: true,
  workers: 2,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:18878',
    trace: 'retain-on-failure',
    launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE },
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    { name: 'phone', use: { ...devices['Pixel 7'], defaultBrowserType: 'chromium' } },
  ],
  webServer: [{
    command: 'node .output/server/index.mjs',
    url: 'http://127.0.0.1:18878',
    reuseExistingServer: false,
    env: { PORT: '18878', HOST: '127.0.0.1', NUXT_PUBLIC_SITE_URL: 'http://127.0.0.1:18878' },
  }, {
    command: 'node .output/server/index.mjs',
    url: 'http://127.0.0.1:18880',
    reuseExistingServer: false,
    env: { PORT: '18880', HOST: '127.0.0.1', NUXT_BILLING_ENABLED: 'true',
      NUXT_BILLING_ORIGIN: 'http://127.0.0.1:18880', NUXT_DATABASE_URL: process.env.TEST_DATABASE_URL ?? '',
      NUXT_STRIPE_SECRET: 'sk_test_local_fixture', NUXT_STRIPE_PRICE: 'price_local_fixture',
      NUXT_STRIPE_WEBHOOK_SECRET: 'whsec_local_fixture', NUXT_SMTP_URL: 'smtp://127.0.0.1:18881',
      NUXT_MAIL_FROM: 'ELIZA <signin@example.test>',
      NUXT_ENTITLEMENT_SECRET: 'local-entitlement-fixture-32-bytes-only',
      NUXT_REPAIR_SECRET: 'local-repair-fixture-32-bytes-only' },
  }],
})
