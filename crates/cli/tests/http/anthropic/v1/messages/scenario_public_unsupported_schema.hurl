POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "Hello"}],
  "output_config": {
    "format": {
      "type": "json_schema",
      "schema": {"type": "number", "minimum": 1}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::anthropic::unsupported_output_schema"
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "invalid_request_error"
body contains "/minimum"
