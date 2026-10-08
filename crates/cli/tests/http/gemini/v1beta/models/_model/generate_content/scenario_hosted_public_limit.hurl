POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.3
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 200
[Asserts]
header "Cache-Control" == "no-store"

POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.3
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"Hello"}]}]}
HTTP 429
[Asserts]
header "Content-Type" startsWith "application/json"
header "Retry-After" exists
header "x-eliza-error-code" == "eliza::admission::rate_limit"
body contains "$3/month"
body contains "https://example.com/support"
jsonpath "$.error.status" == "RESOURCE_EXHAUSTED"
