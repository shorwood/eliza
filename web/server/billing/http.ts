import { timingSafeEqual } from 'node:crypto'
import { isError } from 'h3'
import type { H3Event } from 'h3'
import { BillingError } from './service'
import { billingRuntime } from './runtime'

export const cookie = 'eliza_session'

export function sameOrigin(event: H3Event) {
  if (getHeader(event, 'origin') !== useRuntimeConfig().billingOrigin) {
    throw createError({ statusCode: 403, statusMessage: 'Request origin rejected.' })
  }
}

export function restricted(event: H3Event, expected: string) {
  const received = getHeader(event, 'authorization') ?? ''
  const value = `Bearer ${expected}`
  if (expected.length < 32 || Buffer.byteLength(received) !== Buffer.byteLength(value)
    || !timingSafeEqual(Buffer.from(received), Buffer.from(value))) {
    throw createError({ statusCode: 401, statusMessage: 'Unauthorized.' })
  }
}

export async function limitedBody(event: H3Event, maximum: number) {
  const checkSize = (size: number) => {
    if (size > maximum) {
      // Leave the socket alive long enough to send 413, then close the unread upload.
      setHeader(event, 'Connection', 'close')
      throw new BillingError(413, 'Request body is too large.')
    }
  }
  checkSize(Number(getHeader(event, 'content-length') ?? 0))
  const chunks: Buffer[] = []
  let size = 0
  for await (const chunk of event.node.req.iterator({ destroyOnReturn: false })) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)
    size += bytes.length
    checkSize(size)
    chunks.push(bytes)
  }
  return Buffer.concat(chunks, size).toString('utf8')
}

export async function fields(event: H3Event): Promise<Record<string, unknown>> {
  if (!getHeader(event, 'content-type')?.startsWith('application/json')) throw createError({ statusCode: 415 })
  const raw = await limitedBody(event, 4096)
  if (!raw) throw createError({ statusCode: 413 })
  try {
    const body: unknown = JSON.parse(raw)
    if (!body || typeof body !== 'object' || Array.isArray(body)) throw new Error()
    return body as Record<string, unknown>
  }
  catch { throw createError({ statusCode: 400, statusMessage: 'Invalid JSON.' }) }
}

export function stringField(body: Record<string, unknown>, name: string) {
  const value = body[name]
  if (typeof value !== 'string' || value.length > 512) throw createError({ statusCode: 400, statusMessage: 'Invalid request.' })
  return value
}

export async function safely<T>(event: H3Event, action: () => Promise<T>) {
  setHeader(event, 'Cache-Control', 'private, no-store')
  setHeader(event, 'Referrer-Policy', 'no-referrer')
  try { return await action() }
  catch (error) {
    if (error instanceof BillingError) {
      if (error.status === 429) setHeader(event, 'Retry-After', 900)
      throw createError({ statusCode: error.status, statusMessage: error.message })
    }
    // Avoid framework logs/JSON exposing database connection strings or provider payloads.
    if (isError(error) && error.statusCode < 500) throw error
    throw createError({ statusCode: 503, statusMessage: 'Account service temporarily unavailable.' })
  }
}

export async function authenticated(event: H3Event) {
  return billingRuntime().billing.account(getCookie(event, cookie) ?? '')
}
