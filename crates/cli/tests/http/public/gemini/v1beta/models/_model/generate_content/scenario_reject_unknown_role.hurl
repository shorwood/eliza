POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{"contents": [{"role": "future-role", "parts": [{"text": "Hello"}]}]}
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::gemini::unsupported_role"
jsonpath "$" isObject
jsonpath "$.error.*" count == 3
jsonpath "$.error.code" == 400
jsonpath "$.error.message" == "unsupported Gemini role"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
