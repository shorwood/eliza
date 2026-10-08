POST {{base_url}}/openai/v1/chat/completions
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"chat-two","stream":true,"messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "chat-two"
body contains "[DONE]"
