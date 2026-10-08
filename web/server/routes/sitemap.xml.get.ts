import documents from '../../generated/docs.json'

export default defineEventHandler((event) => {
  const site = useRuntimeConfig(event).public.siteUrl
  if (!site) throw createError({ statusCode: 404, statusMessage: 'Production domain not configured' })
  const routes = ['/', '/terms', '/privacy', ...documents.map(doc => doc.slug === 'index' ? '/docs' : `/docs/${doc.slug}`)]
  setHeader(event, 'Content-Type', 'application/xml; charset=utf-8')
  const urls = routes.map(path => `<url><loc>${new URL(path, site).href.replaceAll('&', '&amp;')}</loc></url>`).join('')
  return `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${urls}</urlset>`
})
