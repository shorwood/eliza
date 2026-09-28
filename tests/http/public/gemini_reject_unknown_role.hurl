POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{"contents": [{"role": "future-role", "parts": [{"text": "Hello"}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::unsupported_role"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
