POST {{base_url}}/ollama/api/chat
Content-Type: application/json
```raw
{
  "model":
}
```
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::extract::json_syntax"
jsonpath "$.error" isString
