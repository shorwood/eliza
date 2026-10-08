POST {{base_url}}/gemini/v1beta/openai/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.5
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 200
[Asserts]
header "Cache-Control" == "no-store"

POST {{base_url}}/gemini/v1beta/openai/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.5
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 429
[Asserts]
header "Content-Type" startsWith "application/json"
header "Retry-After" exists
header "x-eliza-error-code" == "eliza::admission::rate_limit"
body contains "$3/month"
body contains "https://example.com/support"
jsonpath "$.error.type" == "rate_limit_error"
