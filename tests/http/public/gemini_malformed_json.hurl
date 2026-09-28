POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
```raw
{
  "contents":
}
```
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$" isObject
jsonpath "$.error.*" count == 3
jsonpath "$.error.code" == 400
jsonpath "$.error.message" startsWith "invalid JSON syntax:"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
