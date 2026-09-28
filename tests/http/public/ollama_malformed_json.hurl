POST {{base_url}}/ollama/api/chat
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
jsonpath "$.error" startsWith "invalid JSON syntax:"
