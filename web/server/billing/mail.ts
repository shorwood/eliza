import nodemailer from 'nodemailer'

export function loginMailer(connection: string) {
  const url = new URL(connection)
  if (!['smtp:', 'smtps:'].includes(url.protocol) || url.search || url.hash) {
    throw new Error('Use a plain SMTP connection URL without query overrides.')
  }
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)
  return nodemailer.createTransport({ url: connection, requireTLS: !local && url.protocol === 'smtp:',
    tls: { rejectUnauthorized: true }, connectionTimeout: 5000, greetingTimeout: 5000,
    socketTimeout: 10_000, dnsTimeout: 5000, logger: false, debug: false,
    disableFileAccess: true, disableUrlAccess: true })
}
