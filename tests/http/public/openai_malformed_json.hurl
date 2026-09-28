POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
```raw
{
  "model":
}
```
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.code" == "eliza::extract::json_syntax"
