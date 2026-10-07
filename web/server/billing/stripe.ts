import Stripe from 'stripe'
import type { Payments, Snapshot } from './service'

export function testStripe(secret: string) {
  if (!secret.startsWith('sk_test_')) throw new Error('Only Stripe test mode is enabled.')
  return new Stripe(secret, { apiVersion: '2026-09-30.endive', timeout: 10_000, maxNetworkRetries: 1 })
}

export class StripePayments implements Payments {
  constructor(readonly stripe: Stripe, readonly price: string, readonly origin: string) {}

  private async validatePrice() {
    const price = await this.stripe.prices.retrieve(this.price)
    if (price.livemode || !price.active || price.currency !== 'usd' || price.unit_amount !== 500
      || price.recurring?.interval !== 'month' || price.recurring.interval_count !== 1
      || price.recurring.usage_type !== 'licensed') throw new Error('Expected a test-mode $5 USD monthly price.')
  }

  async customer(accountId: string, email: string) {
    return (await this.stripe.customers.create({ email, metadata: { accountId } }, { idempotencyKey: `customer:${accountId}` })).id
  }

  async subscriptions(customer: string): Promise<Snapshot[]> {
    const snapshots: Snapshot[] = []
    for await (const subscription of this.stripe.subscriptions.list({ customer, status: 'all', limit: 100 })) {
      if (snapshots.length >= 100) throw new Error('Subscription scan exceeded its bound.')
      if (subscription.livemode) throw new Error('Live subscription rejected.')
      if (!subscription.items.data.some(item => item.price.id === this.price)) continue
      let paidUntil = 0
      let scanned = 0
      for await (const invoice of this.stripe.invoices.list({ customer, subscription: subscription.id, status: 'paid', limit: 100 })) {
        if (++scanned > 1000) throw new Error('Invoice scan exceeded its bound.')
        if (invoice.livemode || invoice.amount_paid < 500 || invoice.currency !== 'usd') continue
        for await (const line of this.stripe.invoices.listLineItems(invoice.id, { limit: 100 })) {
          if (line.pricing?.price_details?.price === this.price
            && line.parent?.subscription_item_details?.subscription === subscription.id && line.amount >= 500) {
            paidUntil = Math.max(paidUntil, line.period.end * 1000)
          }
        }
        // The newest qualifying paid invoice suffices for this single fixed monthly plan.
        // Stored paid-through is monotonic; normal repair need not rescan years of invoices.
        if (paidUntil) break
      }
      snapshots.push({ id: subscription.id, status: subscription.status, paidUntil })
    }
    return snapshots
  }

  async checkout(customer: string, previous: string | null) {
    await this.validatePrice()
    if (previous) {
      const session = await this.stripe.checkout.sessions.retrieve(previous)
      if (session.customer !== customer || session.livemode) throw new Error('Checkout ownership mismatch.')
      if (session.status === 'open' && session.url) return { id: session.id, url: session.url }
    }
    const session = await this.stripe.checkout.sessions.create({
      mode: 'subscription', customer, line_items: [{ price: this.price, quantity: 1 }],
      success_url: `${this.origin}/account`, cancel_url: `${this.origin}/support`,
      allow_promotion_codes: false,
    }, { idempotencyKey: `checkout:${customer}:${previous || 'initial'}` })
    if (session.livemode || !session.url) throw new Error('Test checkout URL missing.')
    return { id: session.id, url: session.url }
  }

  async portal(customer: string) {
    return (await this.stripe.billingPortal.sessions.create({ customer, return_url: `${this.origin}/account` })).url
  }
}

// Store only routing identifiers, never the full payment payload. Signatures are checked first.
export function webhookCustomer(event: Stripe.Event): string | null {
  const relevant = event.type.startsWith('customer.subscription.') || event.type.startsWith('invoice.')
    || event.type === 'checkout.session.completed' || event.type === 'checkout.session.async_payment_succeeded'
  if (!relevant) return null
  const object = event.data.object
  if (!('customer' in object)) return null
  return typeof object.customer === 'string' ? object.customer : object.customer?.id ?? null
}
