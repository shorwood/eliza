import { authenticated, cookie, fields, safely, sameOrigin, stringField } from '../../billing/http'
import { billingRuntime } from '../../billing/runtime'

export default defineEventHandler(event => safely(event, async () => {
  const action = getRouterParam(event, 'action')
  const config = useRuntimeConfig()
  if (event.method === 'GET' && action === 'state') {
    if (!config.billingEnabled) return { available: false, account: null }
    const billing = billingRuntime().billing
    const session = getCookie(event, cookie)
    if (!session) return { available: true, account: null }
    try {
      const account = await billing.account(session)
      return { available: true, account: { email: account.email, ...await billing.summary(account.id) } }
    }
    catch (error) {
      if (error instanceof Error && 'status' in error && error.status === 401) return { available: true, account: null }
      throw error
    }
  }
  if (event.method !== 'POST') throw createError({ statusCode: 405 })
  sameOrigin(event)
  const billing = billingRuntime().billing
  const body = await fields(event)
  if (action === 'login') {
    // Do not trust X-Forwarded-For supplied by clients. Provider edge limits supplement this.
    await billing.limit('login-ip', getRequestIP(event) ?? 'unknown', 10, 900)
    await billing.login(stringField(body, 'email'))
    return { sent: true }
  }
  if (action === 'confirm') {
    await billing.limit('confirm-ip', getRequestIP(event) ?? 'unknown', 30, 900)
    const session = await billing.consume(stringField(body, 'token'))
    setCookie(event, cookie, session, { httpOnly: true, secure: config.billingOrigin.startsWith('https:'),
      sameSite: 'strict', path: '/', maxAge: 7 * 86400 })
    return { signedIn: true }
  }
  const account = await authenticated(event)
  if (action === 'logout') {
    await billing.logout(getCookie(event, cookie) ?? '')
    deleteCookie(event, cookie, { path: '/' })
    return { signedOut: true }
  }
  if (action === 'keys/create' || action === 'keys/rotate') {
    await billing.limit('key-create', account.id, 20, 900)
    const id = action === 'keys/rotate' ? stringField(body, 'id') : undefined
    if (id && !/^[\da-f-]{36}$/.test(id)) throw createError({ statusCode: 400 })
    return billing.createKey(account.id, stringField(body, 'label'), id)
  }
  if (action === 'keys/revoke') {
    const id = stringField(body, 'id')
    if (!/^[\da-f-]{36}$/.test(id)) throw createError({ statusCode: 400 })
    await billing.revoke(account.id, id)
    return { revoked: true }
  }
  throw createError({ statusCode: 404 })
}))
