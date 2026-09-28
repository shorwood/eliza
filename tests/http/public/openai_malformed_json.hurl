POST {{base_url}}/openai/v1/chat/completions
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
jsonpath "$" isObject
jsonpath "$.error.*" count == 4
jsonpath "$.error.message" startsWith "invalid JSON syntax:"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == null
jsonpath "$.error.code" == "eliza::extract::json_syntax"
