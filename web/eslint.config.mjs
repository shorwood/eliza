import withNuxt from './.nuxt/eslint.config.mjs'

export default withNuxt({
  ignores: ['generated/**', 'public/examples/**', 'test-results/**', 'playwright-report/**'],
})
