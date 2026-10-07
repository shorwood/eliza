export interface AccountState {
  available: boolean
  account: null | {
    email: string
    eligible: boolean
    eligibleUntil: number | null
    keys: { id: string, label: string, created_at: string, expires_at: string | null, revoked_at: string | null }[]
  }
}
