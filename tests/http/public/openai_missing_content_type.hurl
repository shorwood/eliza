POST {{base_url}}/openai/v1/chat/completions
```raw
{
  "model": "eliza-doctor",
  "messages": [{"role": "user", "content": "Hello"}]
}
```
HTTP 415
[Asserts]
header "x-eliza-error-code" == "eliza::extract::missing_json_content_type"
jsonpath "$.error.type" == "invalid_request_error"
jsonpath "$.error.code" == "eliza::extract::missing_json_content_type"
