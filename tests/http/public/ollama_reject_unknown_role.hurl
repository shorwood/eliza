POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-doctor",
  "stream": false,
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
jsonpath "$.error" isString
