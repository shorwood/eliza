POST {{base_url}}/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse&alt=json
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::extract::query"
jsonpath "$.error.code" == 400
jsonpath "$.error.status" == "INVALID_ARGUMENT"
