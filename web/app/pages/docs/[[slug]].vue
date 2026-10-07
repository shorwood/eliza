<script setup lang="ts">
import documents from '../../../generated/docs.json'

const route = useRoute()
const slug = String(route.params.slug || 'index')
const document = documents.find(doc => doc.slug === slug)
if (!document) throw createError({ statusCode: 404, statusMessage: 'Documentation page not found' })
useSeoMeta({ title: `${document.title} · ELIZA` })
</script>

<template>
  <div class="docs-layout">
    <aside class="docs-sidebar">
      <p class="eyebrow">Documentation</p>
      <nav aria-label="Documentation">
        <a v-for="doc in documents" :key="doc.slug" :href="doc.slug === 'index' ? '/docs' : `/docs/${doc.slug}`" :aria-current="doc.slug === slug ? 'page' : undefined">{{ doc.slug === 'index' ? 'Overview' : doc.title }}</a>
      </nav>
      <a class="raw-link" :href="`/docs/${slug}.md`">View Markdown ↗</a>
      <a class="raw-link" href="/openapi.json">OpenAPI JSON ↗</a>
    </aside>
    <!-- Repository Markdown is sanitized at build time; no user HTML is rendered. -->
    <!-- eslint-disable-next-line vue/no-v-html -->
    <article class="prose" v-html="document.html" />
  </div>
</template>
