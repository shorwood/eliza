<script setup lang="ts">
import type { AccountState } from '../../shared/types/billing'
useSeoMeta({ title: 'Support ELIZA', robots: 'noindex, nofollow' })
const { data: state } = await useFetch<AccountState>('/account/state')
const busy = ref(false)
const message = ref('')
async function checkout() {
  busy.value = true
  try {
    const response = await $fetch<{ url: string }>('/billing/checkout', { method: 'POST' })
    location.assign(response.url)
  }
  catch { message.value = 'Checkout is unavailable. If you already subscribed, manage billing from your account.' }
  finally { busy.value = false }
}
</script>

<template>
  <section class="narrow-page">
    <p class="eyebrow">For builders who rely on the endpoint</p>
    <h1>Support ELIZA.</h1>
    <p class="lead">Help fund a dependable public API for development and CI.</p>
    <p class="support-price">$5 <span>USD / month, recurring</span></p>
    <ul class="benefits">
      <li>Higher limits for your team's CI bursts.</li>
      <li>Capacity protected from anonymous traffic.</li>
      <li>Team keys with rotation and account access.</li>
      <li>The same API capabilities as public access.</li>
    </ul>
    <div v-if="state?.available" class="availability-note">
      <h2>Test checkout</h2>
      <p>Test mode only. This does not grant hosted API access. No real payment can be taken.</p>
      <button v-if="state.account" :disabled="busy" @click="checkout">Open test checkout</button>
      <a v-else href="/account">Sign in to test checkout →</a>
      <p role="status">{{ message }}</p>
    </div>
    <div v-else class="availability-note">
      <h2>Subscriptions are not open yet</h2>
      <p>These benefits are planned. Checkout and account access will open after billing and protected capacity are ready. This page cannot take a payment.</p>
      <button type="button" disabled>Checkout unavailable</button>
    </div>
    <p>Before checkout opens, we'll publish the exact limits and applicable taxes. Canceling will stop renewal and retain access through the paid period. Failed renewals will have seven days' grace. There will be no automatic usage overages.</p>
    <p>Fixture versions will be announced and pinned explicitly. Measured uptime and incidents will be public; an uninterrupted-workflow guarantee is not offered.</p>
    <p><a href="/account">Account access</a> · <a href="/docs">Back to documentation</a></p>
  </section>
</template>
