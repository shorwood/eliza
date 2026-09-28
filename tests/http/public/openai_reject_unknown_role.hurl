POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_message_role"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.code" == "eliza::openai::unsupported_message_role"
