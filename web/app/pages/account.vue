<script setup lang="ts">
import type { AccountState } from '../../shared/types/billing'
useSeoMeta({ title: 'Account access · ELIZA', robots: 'noindex, nofollow' })
useHead({ meta: [{ name: 'referrer', content: 'no-referrer' }] })
const { data: state, refresh, error: loadError } = await useFetch<AccountState>('/account/state')
const email = ref('')
const label = ref('CI')
const loginToken = ref('')
const revealedKey = ref('')
const message = ref('')
const busy = ref(false)
onMounted(() => {
  const fragment = new URLSearchParams(location.hash.slice(1))
  loginToken.value = fragment.get('login') ?? ''
  history.replaceState(null, '', location.pathname)
})
async function perform(action: () => Promise<void>) {
  busy.value = true
  message.value = ''
  try { await action() }
  catch (error) {
    const failure = error as { data?: { statusMessage?: string } }
    message.value = failure.data?.statusMessage ?? 'Could not complete this action. Please try again.'
  }
  finally { busy.value = false }
}
async function login() {
  await perform(async () => {
    await $fetch('/account/login', { method: 'POST', body: { email: email.value } })
    message.value = 'Check your email for a sign-in link. It expires in 15 minutes.'
  })
}
async function confirm() {
  await perform(async () => {
    await $fetch('/account/confirm', { method: 'POST', body: { token: loginToken.value } })
    loginToken.value = ''
    await refresh()
  })
}
async function keyAction(action: 'create' | 'rotate' | 'revoke', id?: string) {
  await perform(async () => {
    const response = await $fetch<{ value?: string }>(`/account/keys/${action}`, { method: 'POST', body: { label: label.value, id } })
    revealedKey.value = response.value ?? ''
    message.value = action === 'rotate' ? 'New key created. The old key expires within 24 hours; revoke it sooner after updating CI.' : action === 'revoke' ? 'Key revoked. API nodes will refresh within 60 seconds once hosted access opens.' : ''
    await refresh()
  })
}
async function portal() {
  await perform(async () => {
    const response = await $fetch<{ url: string }>('/billing/portal', { method: 'POST' })
    location.assign(response.url)
  })
}
async function logout() {
  await perform(async () => {
    await $fetch('/account/logout', { method: 'POST', body: {} })
    revealedKey.value = ''
    await refresh()
  })
}
</script>

<template>
  <section class="narrow-page">
    <p class="eyebrow">Account access</p>
    <h1>Your team,<br>your API keys.</h1>
    <p v-if="loadError" role="alert">Account service is temporarily unavailable. <button @click="refresh()">Try again</button></p>
    <div v-else-if="!state?.available" class="availability-note">
      <h2>Sign-in is being prepared</h2>
      <p>No email address or credential is collected on this preview page.</p>
      <button type="button" disabled>Sign-in unavailable</button>
    </div>
    <template v-else>
      <p class="availability-note">Test mode. These keys do not grant hosted API access yet.</p>
      <form v-if="loginToken" @submit.prevent="confirm">
        <p>Confirm sign-in to use this single-use email link.</p>
        <button :disabled="busy">Confirm sign-in</button>
      </form>
      <form v-else-if="!state.account" @submit.prevent="login">
        <label for="email">Email address</label>
        <input id="email" v-model="email" type="email" autocomplete="email" required maxlength="254">
        <button :disabled="busy">Email a sign-in link</button>
      </form>
      <template v-if="state.account">
        <p>Signed in as {{ state.account.email }}. <button :disabled="busy" @click="logout">Sign out</button></p>
        <p v-if="state.account.eligible">Supporter access through {{ new Date(state.account.eligibleUntil!).toLocaleDateString('en-US', { timeZone: 'UTC' }) }} (UTC).</p>
        <p v-else>No confirmed supporter subscription. Returning from checkout does not activate access.</p>
        <p><button :disabled="busy" @click="portal">Manage billing</button> · <a href="/support">Support ELIZA</a></p>
        <form v-if="state.account.eligible" @submit.prevent="keyAction('create')">
          <label for="label">Key label</label>
          <input id="label" v-model="label" maxlength="80" required>
          <button :disabled="busy">Create key</button>
        </form>
        <div v-if="revealedKey" class="availability-note">
          <h2>Save this key now</h2>
          <p>It is shown once. Store it in your CI secret manager.</p>
          <code class="secret-key">{{ revealedKey }}</code>
          <button @click="revealedKey = ''">Hide key</button>
        </div>
        <ul class="key-list">
          <li v-for="key in state.account.keys" :key="key.id">
            <strong>{{ key.label }}</strong>
            <small>{{ key.id }}</small>
            <span v-if="key.revoked_at">Revoked</span>
            <span v-else-if="key.expires_at && +new Date(key.expires_at) <= Date.now()">Expired</span>
            <template v-else>
              <span v-if="key.expires_at">Expires {{ new Date(key.expires_at).toLocaleString('en-US', { timeZone: 'UTC' }) }} UTC</span>
              <div class="key-actions">
                <button :disabled="busy || !state.account.eligible" @click="keyAction('rotate', key.id)">Rotate {{ key.label }}</button>
                <button :disabled="busy" @click="keyAction('revoke', key.id)">Revoke {{ key.label }}</button>
              </div>
            </template>
          </li>
        </ul>
      </template>
      <p role="status">{{ message }}</p>
    </template>
    <a href="/docs">Return to documentation →</a>
  </section>
</template>
