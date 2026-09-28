POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
```raw
{
  "model":
}
```
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$.type" == "error"
jsonpath "$.error.type" == "invalid_request_error"
