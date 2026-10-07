import { authenticated, restricted, safely, sameOrigin } from '../../billing/http'
import { billingRuntime } from '../../billing/runtime'
import { webhookCustomer } from '../../billing/stripe'

export default defineEventHandler(event => safely(event, async () => {
  const action = getRouterParam(event, 'action')
  if (event.method !== 'POST') throw createError({ statusCode: 405 })
  const config = useRuntimeConfig()
  if (action === 'entitlement') {
    restricted(event, config.entitlementSecret)
    const raw = await readRawBody(event)
    // Hash only: raw keys must never appear in URLs, access logs or this database query.
    if (!raw || !/^[\da-f]{64}$/.test(raw)) throw createError({ statusCode: 400 })
    return billingRuntime().billing.entitlement(raw)
  }
  if (action === 'repair') {
    restricted(event, config.repairSecret)
    return billingRuntime().billing.repair()
  }
  if (action === 'webhook') {
    const { billing, stripe } = billingRuntime()
    const raw = await readRawBody(event)
    if (!raw || Buffer.byteLength(raw) > 1_048_576) throw createError({ statusCode: 413 })
    let notification
    try { notification = stripe.webhooks.constructEvent(raw, getHeader(event, 'stripe-signature') ?? '', config.stripeWebhookSecret) }
    catch { throw createError({ statusCode: 400, statusMessage: 'Invalid webhook signature.' }) }
    if (notification.livemode) throw createError({ statusCode: 400, statusMessage: 'Live event rejected.' })
    const customer = webhookCustomer(notification)
    if (customer) await billing.enqueue(notification.id, customer)
    // Ack only after durable insert. A scheduled repair processes pending events, including crashes.
    return { received: true }
  }
  sameOrigin(event)
  const account = await authenticated(event)
  const billing = billingRuntime().billing
  if (action === 'checkout') {
    await billing.limit('checkout', account.id, 5, 900)
    return { url: await billing.checkout(account.id) }
  }
  if (action === 'portal') {
    await billing.limit('portal', account.id, 10, 900)
    return { url: await billing.portal(account.id) }
  }
  throw createError({ statusCode: 404 })
}))
