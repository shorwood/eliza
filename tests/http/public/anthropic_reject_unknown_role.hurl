POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::anthropic::unsupported_role"
jsonpath "$.*" count == 2
jsonpath "$.type" == "error"
jsonpath "$.error.*" count == 2
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.message" == "unsupported Anthropic role"
