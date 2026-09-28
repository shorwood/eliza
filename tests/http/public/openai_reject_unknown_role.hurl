POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
jsonpath "$.error.type" == "unsupported_request_error"
