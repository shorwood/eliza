POST {{base_url}}/openai/v1/chat/completions
```raw
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}]
}
```
HTTP 415
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::extract::missing_json_content_type"
jsonpath "$" isObject
jsonpath "$.error.*" count == 4
jsonpath "$.error.message" == "expected request with `Content-Type: application/json`"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.param" == null
jsonpath "$.error.code" == "eliza::extract::missing_json_content_type"
