POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-doctor",
  "stream": false,
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::unsupported_role"
jsonpath "$.error" isString
