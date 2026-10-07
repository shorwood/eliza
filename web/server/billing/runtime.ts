import postgres from 'postgres'
import { loginMailer } from './mail'
import { Billing } from './service'
import { StripePayments, testStripe } from './stripe'

let runtime: ReturnType<typeof createRuntime> | undefined

function createRuntime(config: ReturnType<typeof useRuntimeConfig>) {
  const origin = new URL(config.billingOrigin)
  if (origin.origin !== config.billingOrigin || (origin.protocol !== 'https:' && !['localhost', '127.0.0.1'].includes(origin.hostname))) {
    throw new Error('Billing origin must be an HTTPS origin (HTTP loopback allowed locally).')
  }
  if (!config.databaseUrl || !config.stripePrice || !config.stripeWebhookSecret || !config.smtpUrl || !config.mailFrom
    || config.entitlementSecret.length < 32 || config.repairSecret.length < 32) throw new Error('Billing configuration is incomplete.')
  const stripe = testStripe(config.stripeSecret)
  const mail = loginMailer(config.smtpUrl)
  const sql = postgres(config.databaseUrl, { max: 4, idle_timeout: 20, connect_timeout: 5,
    connection: { statement_timeout: 30_000 }, onnotice: () => {} })
  const billing = new Billing(sql, new StripePayments(stripe, config.stripePrice, origin.origin), async (email, token) => {
    await mail.sendMail({ from: config.mailFrom, to: email, subject: 'Sign in to ELIZA',
      text: `Use this link within 15 minutes to sign in. Confirm sign-in on the page; the link can be used once.\n\n${origin.origin}/account#login=${token}\n\nIf you did not request this email, ignore it.` })
  })
  return { billing, stripe }
}

export function billingRuntime() {
  const config = useRuntimeConfig()
  if (!config.billingEnabled) throw createError({ statusCode: 503, statusMessage: 'Accounts are not available yet.' })
  return runtime ??= createRuntime(config)
}
