POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "unsupported_request_error"
