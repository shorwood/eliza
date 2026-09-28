POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 401
[Asserts]
jsonpath "$.error.type" == "authentication_error"
