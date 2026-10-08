POST {{base_url}}/openai/v1/chat/completions
x-eliza-ingress-token: test-ingress-secret-at-least-32-bytes
x-eliza-client-ip: 192.0.2.30
Authorization: Bearer eliza_test_00000000-0000-0000-0000-000000000003_CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 401
[Asserts]
jsonpath "$.error.type" == "authentication_error"
body not contains "$3"
