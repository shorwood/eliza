POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
```raw
{
  "model":
}
```
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$.*" count == 2
jsonpath "$.type" == "error"
jsonpath "$.error.*" count == 2
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.message" startsWith "invalid JSON syntax:"
