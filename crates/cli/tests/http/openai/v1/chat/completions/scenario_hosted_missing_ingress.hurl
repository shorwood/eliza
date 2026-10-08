POST {{base_url}}/openai/v1/chat/completions
x-forwarded-for: 192.0.2.99
Content-Type: application/json
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 401
[Asserts]
jsonpath "$.error.type" == "authentication_error"
