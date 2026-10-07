<script setup lang="ts">
useSeoMeta({ title: 'Privacy · ELIZA' })
</script>

<template>
  <section class="narrow-page prose">
    <h1>Privacy during preview</h1>
    <p>This application has no account sign-in, payment collection, optional analytics, or tracking cookies. The landing page displays examples; it sends no generation request.</p>
    <p>Requests to this site can still create hosting-provider access logs. There is no deployed hosting provider or production retention policy to announce yet.</p>
    <h2>Before launch</h2>
    <p>The privacy policy must identify the operator, processors, regions, retention periods, backups, and contact/deletion process. It must match the hosting and billing configuration.</p>
    <p>The intended service will keep conversation state request-local and avoid application logs containing prompts, generated media, raw keys, or authorization headers.</p>
  </section>
</template>
