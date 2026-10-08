POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "input": "Hello",
  "text": {
    "format": {
      "type": "json_schema",
      "name": "bad",
      "schema": {"type": "object", "required": ["missing"]}
    }
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::invalid_response_schema"
jsonpath "$.error.param" == "text.format"
body contains "/required"
