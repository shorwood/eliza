POST {{base_url}}/ollama/api/chat
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-one","stream":false,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-one"
