import { cp, mkdir, readFile, readdir, realpath, rm, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { marked } from 'marked'
import sanitize from 'sanitize-html'
import { publicExamples } from './public-examples.mjs'

for (const name of ['NUXT_PUBLIC_API_ROOT', 'NUXT_PUBLIC_SITE_URL', 'NUXT_PUBLIC_STATUS_URL']) {
  const value = process.env[name]
  if (!value) continue
  const url = new URL(value)
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
    throw new Error(`${name} must be an HTTP(S) URL without credentials, query, or fragment`)
  }
  if (name === 'NUXT_PUBLIC_SITE_URL' && url.pathname !== '/') throw new Error('NUXT_PUBLIC_SITE_URL must be an origin')
}

const web = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const generated = join(web, 'generated')
await rm(generated, { recursive: true, force: true })
await mkdir(join(generated, 'public/docs'), { recursive: true })
const documents = []
for (const name of (await readdir(join(web, 'content/docs'))).sort()) {
  if (!name.endsWith('.md')) continue
  const source = join(web, 'content/docs', name)
  if (await realpath(source) !== source) throw new Error(`Symlink rejected: ${name}`)
  const markdown = await readFile(source, 'utf8')
  const title = markdown.match(/^# (.+)$/m)?.[1]
  if (!title) throw new Error(`Missing title: ${name}`)
  const html = sanitize(await marked.parse(markdown), {
    transformTags: {
      a: (tagName, attributes) => {
        let href = attributes.href ?? ''
        if (/^[a-z-]+\.md$/.test(href)) {
          href = `/docs/${href.slice(0, -3) === 'index' ? '' : href.slice(0, -3)}`
        } else if (href.startsWith('../examples/')) {
          href = href.slice(2)
        }
        return { tagName, attribs: { ...attributes, href } }
      },
    },
  })
  documents.push({ slug: name.slice(0, -3), title, html })
  await cp(source, join(generated, 'public/docs', name))
}
await writeFile(join(generated, 'docs.json'), `${JSON.stringify(documents)}\n`)
for (const name of publicExamples) {
  const source = join(web, 'public/examples', name)
  if (await realpath(source) !== source) throw new Error(`Symlink rejected: ${name}`)
  const target = join(generated, 'public/examples', name)
  await mkdir(dirname(target), { recursive: true })
  await cp(source, target)
}
await cp(join(web, 'public/openapi.json'), join(generated, 'public/openapi.json'))
await cp(join(web, 'PUBLIC-LICENSE'), join(generated, 'public/LICENSE'))
await writeFile(join(generated, 'public/llms.txt'), '# ELIZA API fixture\n\nHosted access is pending.\n\n'
  + '- [OpenAPI](/openapi.json)\n'
  + documents.map(doc => `- [${doc.title}](/docs/${doc.slug}.md)`).join('\n') + '\n')
