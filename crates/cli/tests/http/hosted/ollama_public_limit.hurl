POST {{base_url}}/ollama/api/chat
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.4
Content-Type: application/json
{"model":"eliza-1966","stream":false,"messages":[{"role":"user","content":"Hello"}]}
HTTP 200
[Asserts]
header "Cache-Control" == "no-store"

POST {{base_url}}/ollama/api/chat
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.4
Content-Type: application/json
{"model":"eliza-1966","stream":false,"messages":[{"role":"user","content":"Hello"}]}
HTTP 429
[Asserts]
header "Content-Type" startsWith "application/json"
header "Retry-After" exists
header "x-eliza-error-code" == "eliza::admission::rate_limit"
body contains "$5/month"
body contains "https://example.com/support"
