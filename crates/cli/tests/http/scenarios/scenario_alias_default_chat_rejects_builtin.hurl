# Configured chat names replace the built-in name on OpenAI routes.
POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
Authorization: Bearer {{token}}
{"model":"eliza-1966","messages":[{"role":"user","content":"Hello"}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::chat::model_required"
jsonpath "$.error.message" contains "chat-one"
jsonpath "$.error.message" contains "chat-two"
