POST {{base_url}}/anthropic/v1/messages
x-api-key: {{token}}
Content-Type: application/json
{"model":"chat-two","max_tokens":128,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-two"
