<script setup lang="ts">
useSeoMeta({ title: 'ELIZA · Test your AI integration' })
const config = useRuntimeConfig()
const choices = [
  { name: 'OpenAI', path: '/openai/v1/chat/completions', body: { model: 'eliza-1966', messages: [{ role: 'user', content: 'I am sad.' }] } },
  { name: 'Anthropic', path: '/anthropic/v1/messages', body: { model: 'eliza-1966', max_tokens: 128, messages: [{ role: 'user', content: 'I am sad.' }] } },
  { name: 'Gemini', path: '/gemini/v1beta/models/eliza-1966:generateContent', body: { contents: [{ parts: [{ text: 'I am sad.' }] }] } },
  { name: 'Ollama', path: '/ollama/api/chat', body: { model: 'eliza-1966', stream: false, messages: [{ role: 'user', content: 'I am sad.' }] } },
]
const selected = ref(0)
const feedback = ref('')
const command = computed(() => {
  const provider = choices[selected.value]!
  const root = config.public.apiRoot.replace(/\/$/, '')
  const version = provider.name === 'Anthropic' ? "\n  -H 'anthropic-version: 2023-06-01' \\" : ''
  return `curl --fail-with-body --connect-timeout 5 --max-time 15 \\\n  '${root}${provider.path}' \\\n  -H 'Content-Type: application/json' \\${version}\n  -d '${JSON.stringify(provider.body)}'`
})
watch(selected, () => { feedback.value = '' })
async function copyCommand() {
  try {
    await navigator.clipboard.writeText(command.value)
    feedback.value = 'Command copied.'
  } catch {
    feedback.value = 'Select and copy the command below.'
  }
}
</script>

<template>
  <div>
  <section class="landing">
    <div class="intro">
      <p class="eyebrow">A developer fixture with a little history</p>
      <h1>A test endpoint<br>for your AI client.</h1>
      <p class="lead">Exercise chat, streaming, tools and synthetic media with repeatable responses.</p>
      <a class="text-link" href="/docs">Read the documentation <span aria-hidden="true">→</span></a>
      <p v-if="!config.public.serviceLive" class="preview-note"><span class="status-dot" />Hosted access is being prepared. These examples use a local instance.</p>
      <p v-else class="preview-note">Public access requires no signup.</p>
    </div>
    <div class="request-panel">
      <div class="request-toolbar">
        <div><label for="provider">Provider</label><select id="provider" v-model="selected"><option v-for="(choice, index) in choices" :key="choice.name" :value="index">{{ choice.name }}</option></select></div>
        <button type="button" @click="copyCommand">Copy request</button>
      </div>
      <pre tabindex="0" aria-label="Example curl request"><code>{{ command }}</code></pre>
      <p class="copy-feedback" role="status">{{ feedback }}</p>
      <div class="sample-reply"><span>ELIZA replies</span><code>I AM SORRY TO HEAR YOU ARE SAD</code></div>
    </div>
  </section>
  <section class="capabilities" aria-label="Fixture capabilities">
    <div><h2>Use your SDK</h2><p>Provider-shaped requests and responses for OpenAI, Anthropic, Gemini and Ollama clients.</p></div>
    <div><h2>Test the transport</h2><p>Streaming, structured output and deterministic tool calls for integration tests.</p></div>
    <div><h2>Keep it repeatable</h2><p>Classic chat, retro speech, hashed embeddings and procedural images.</p></div>
  </section>
  </div>
</template>
