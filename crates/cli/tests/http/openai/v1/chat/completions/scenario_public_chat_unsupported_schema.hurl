POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "Hello"}],
  "response_format": {
    "type": "json_schema",
    "json_schema": {
      "name": "bad",
      "schema": {"type": "string", "pattern": "x"}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_response_schema"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == "response_format"
body contains "/pattern"
