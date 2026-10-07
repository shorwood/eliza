<script setup lang="ts">
useSeoMeta({ title: 'Privacy · ELIZA' })
</script>

<template>
  <section class="narrow-page prose">
    <h1>Privacy during preview</h1>
    <p>Account access is disabled by default, and real payments are not accepted. There are no optional analytics or tracking cookies. The landing page displays examples; it sends no generation request.</p>
    <p>When test accounts are enabled, sign-in collects an email address and sends a single-use link. A necessary session cookie lasts up to seven days. Account records hold email addresses, billing identifiers and key labels; API keys and sign-in credentials are stored as hashes. Keys are never emailed.</p>
    <p>Requests to this site can still create hosting-provider access logs. There is no deployed hosting provider or production retention policy to announce yet.</p>
    <h2>Before launch</h2>
    <p>The privacy policy must identify the operator, processors, regions, retention periods, backups, and contact/deletion process. It must match the hosting and billing configuration.</p>
    <p>The intended service will keep conversation state request-local and avoid application logs containing prompts, generated media, raw keys, or authorization headers.</p>
  </section>
</template>
