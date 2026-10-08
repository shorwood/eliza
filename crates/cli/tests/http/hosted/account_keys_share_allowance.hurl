POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.20
Authorization: Bearer eliza_test_00000000-0000-0000-0000-000000000001_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 200

POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.20
Authorization: Bearer eliza_test_00000000-0000-0000-0000-000000000002_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 200

POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.20
Authorization: Bearer eliza_test_00000000-0000-0000-0000-000000000001_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 429
[Asserts]
body not contains "$5"
jsonpath "$.error.type" == "rate_limit_error"
header "Retry-After" exists

