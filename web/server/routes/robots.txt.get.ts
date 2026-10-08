export default defineEventHandler((event) => {
  const site = useRuntimeConfig(event).public.siteUrl
  setHeader(event, 'Content-Type', 'text/plain; charset=utf-8')
  if (!site) return 'User-agent: *\nDisallow: /\n'
  return `User-agent: *\nDisallow: /account\nDisallow: /support\nDisallow: /billing\nSitemap: ${new URL('/sitemap.xml', site).href}\n`
})
