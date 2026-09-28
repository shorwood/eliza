POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
```raw
{
  "contents":
}
```
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$.error.code" == 400
jsonpath "$.error.status" == "INVALID_ARGUMENT"
