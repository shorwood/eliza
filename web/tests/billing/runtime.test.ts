import { afterEach, expect, test, vi } from 'vitest'
import { billingRuntime } from '../../server/billing/runtime'

afterEach(() => { vi.unstubAllGlobals() })

test('billing accepts only HTTPS origins or explicit HTTP loopback', () => {
  for (const billingOrigin of ['ftp://localhost', 'http://remote.example', 'https://example.test/path']) {
    vi.stubGlobal('useRuntimeConfig', () => ({ billingEnabled: true, billingOrigin }))
    expect(() => billingRuntime()).toThrow('HTTPS origin')
  }
  for (const billingOrigin of ['https://example.test', 'http://127.0.0.1:8787', 'http://[::1]:8787']) {
    vi.stubGlobal('useRuntimeConfig', () => ({ billingEnabled: true, billingOrigin }))
    expect(() => billingRuntime()).toThrow('configuration is incomplete')
  }
})
