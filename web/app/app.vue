<script setup lang="ts">
const route = useRoute()
const config = useRuntimeConfig()
useSeoMeta({ description: 'Repeatable responses for testing AI SDK integrations with ELIZA.' })
useHead(() => ({
  link: config.public.siteUrl
    ? [{ rel: 'canonical', href: new URL(route.path, config.public.siteUrl).href }]
    : [],
}))
</script>

<template>
  <div class="site-shell">
    <a class="skip-link" href="#main">Skip to content</a>
    <header class="site-header">
      <a class="wordmark" href="/" aria-label="ELIZA home">ELIZA<span> / api</span></a>
      <nav aria-label="Main navigation">
        <a href="/docs">Docs</a>
        <a href="https://github.com/shorwood/eliza-docs">Examples ↗</a>
      </nav>
    </header>
    <main id="main" tabindex="-1"><NuxtPage /></main>
    <footer class="site-footer">
      <span>Powered by ELIZA / DOCTOR, 1966.</span>
      <nav aria-label="Footer navigation">
        <a href="/terms">Terms</a>
        <a href="/privacy">Privacy</a>
        <a v-if="config.public.statusUrl" :href="config.public.statusUrl">Status ↗</a>
        <a v-else href="/docs/service">Service status</a>
      </nav>
    </footer>
  </div>
</template>
