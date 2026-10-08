POST {{base_url}}/openai/v1/audio/speech
Content-Type: application/json
```raw
{"model":
```
HTTP 401
[Asserts]
header "x-eliza-error-code" == "eliza::auth::failed"
jsonpath "$.error.type" == "authentication_error"
