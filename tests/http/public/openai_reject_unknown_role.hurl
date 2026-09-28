POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "messages": [{"role": "future-role", "content": "Hello"}]
}
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::openai::unsupported_message_role"
jsonpath "$" isObject
jsonpath "$.error.*" count == 4
jsonpath "$.error.message" == "unsupported message role"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "messages.role"
jsonpath "$.error.code" == "eliza::openai::unsupported_message_role"
