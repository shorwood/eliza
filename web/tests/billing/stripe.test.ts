import type Stripe from 'stripe'
import { expect, test, vi } from 'vitest'
import { StripePayments, testStripe } from '../../server/billing/stripe'

const page = <T>(values: T[]) => ({ async *[Symbol.asyncIterator]() { yield* values } })
const price = { id: 'price_test', livemode: false, active: true, currency: 'usd', unit_amount: 500,
  recurring: { interval: 'month', interval_count: 1, usage_type: 'licensed' } }

test('only a matching confirmed paid subscription line advances access', async () => {
  const stripe = testStripe('sk_test_fixture')
  const payments = new StripePayments(stripe, 'price_test', 'https://fixture.example')
  vi.spyOn(stripe.subscriptions, 'list').mockReturnValue(page([
    { id: 'sub_test', livemode: false, status: 'past_due', items: { data: [{ price: { id: 'price_test' } }] } },
  ]) as ReturnType<typeof stripe.subscriptions.list>)
  const invoices = vi.spyOn(stripe.invoices, 'list').mockReturnValue(page([
    { id: 'in_zero', amount_paid: 0, currency: 'usd', livemode: false },
    { id: 'in_paid', amount_paid: 500, currency: 'usd', livemode: false },
    { id: 'in_older', amount_paid: 500, currency: 'usd', livemode: false },
  ]) as ReturnType<typeof stripe.invoices.list>)
  const lines = vi.spyOn(stripe.invoices, 'listLineItems').mockReturnValue(page([
    { amount: 500, pricing: { price_details: { price: 'price_other' } }, parent: { subscription_item_details: { subscription: 'sub_test' } }, period: { end: 9999 } },
    { amount: 500, pricing: { price_details: { price: 'price_test' } }, parent: { subscription_item_details: { subscription: 'sub_other' } }, period: { end: 9999 } },
    { amount: 500, pricing: { price_details: { price: 'price_test' } }, parent: { subscription_item_details: { subscription: 'sub_test' } }, period: { end: 1234 } },
  ]) as ReturnType<typeof stripe.invoices.listLineItems>)
  expect(await payments.subscriptions('cus_owner')).toEqual([{ id: 'sub_test', status: 'past_due', paidUntil: 1234000 }])
  expect(invoices).toHaveBeenCalledWith({ customer: 'cus_owner', subscription: 'sub_test', status: 'paid', limit: 100 })
  expect(lines).toHaveBeenCalledTimes(1)
  expect(lines).toHaveBeenCalledWith('in_paid', { limit: 100 })
})

test('server selects price, owner and return URLs; open checkout is reused', async () => {
  const stripe = testStripe('sk_test_fixture')
  const payments = new StripePayments(stripe, 'price_test', 'https://fixture.example')
  vi.spyOn(stripe.prices, 'retrieve').mockResolvedValue(price as Stripe.Response<Stripe.Price>)
  const create = vi.spyOn(stripe.checkout.sessions, 'create').mockResolvedValue({ id: 'cs_test', url: 'https://checkout.stripe.com/test', livemode: false } as Stripe.Response<Stripe.Checkout.Session>)
  expect(await payments.checkout('cus_owner', null)).toMatchObject({ id: 'cs_test' })
  expect(create).toHaveBeenCalledWith({ mode: 'subscription', customer: 'cus_owner',
    line_items: [{ price: 'price_test', quantity: 1 }], success_url: 'https://fixture.example/account',
    cancel_url: 'https://fixture.example/support', allow_promotion_codes: false },
  { idempotencyKey: 'checkout:cus_owner:initial' })
  const retrieve = vi.spyOn(stripe.checkout.sessions, 'retrieve').mockResolvedValue({ id: 'cs_test', customer: 'cus_owner',
    status: 'open', url: 'https://checkout.stripe.com/test', livemode: false } as Stripe.Response<Stripe.Checkout.Session>)
  await payments.checkout('cus_owner', 'cs_test')
  expect(create).toHaveBeenCalledTimes(1)
  retrieve.mockResolvedValueOnce({ customer: 'cus_other', livemode: false } as Stripe.Response<Stripe.Checkout.Session>)
  await expect(payments.checkout('cus_owner', 'cs_stolen')).rejects.toThrow('ownership')
})

test('wrong amount, live prices and nonmonthly plans cannot open checkout', async () => {
  for (const invalid of [{ unit_amount: 1900 }, { livemode: true }, { recurring: { interval: 'year', interval_count: 1 } }]) {
    const stripe = testStripe('sk_test_fixture')
    vi.spyOn(stripe.prices, 'retrieve').mockResolvedValue({ ...price, ...invalid } as Stripe.Response<Stripe.Price>)
    await expect(new StripePayments(stripe, 'price_test', 'https://fixture.example').checkout('cus_owner', null)).rejects.toThrow('test-mode $5')
  }
})
