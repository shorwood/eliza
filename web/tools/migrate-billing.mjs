import { readFile } from 'node:fs/promises'
import postgres from 'postgres'

if (!process.env.NUXT_DATABASE_URL) throw new Error('NUXT_DATABASE_URL is required.')
const sql = postgres(process.env.NUXT_DATABASE_URL, { max: 1, onnotice: () => {} })
try { await sql.unsafe(await readFile(new URL('../server/billing/schema.sql', import.meta.url), 'utf8')) }
finally { await sql.end() }
