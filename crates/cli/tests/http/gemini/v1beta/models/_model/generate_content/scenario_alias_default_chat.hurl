POST {{base_url}}/gemini/v1beta/models/chat-two:generateContent
x-goog-api-key: {{token}}
Content-Type: application/json
{"contents":[{"role":"user","parts":[{"text":"I need help"}]}]}
HTTP 200
[Asserts]
jsonpath "$.modelVersion" == "chat-two"
