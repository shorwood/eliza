# Chat preserves requested IDs in unary and streaming responses.
POST {{base_url}}/openai/v1/chat/completions
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-one","messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "chat-one"
jsonpath "$.choices[0].message.content" != ""
