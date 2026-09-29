POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{
    "role": "user",
    "parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]
  }]
}
HTTP 400
[Asserts]
header "Content-Type" startsWith "application/json"
header "x-eliza-error-code" == "eliza::gemini::unsupported_user_part"
jsonpath "$" isObject
jsonpath "$.error.*" count == 3
jsonpath "$.error.code" == 400
jsonpath "$.error.message" == "only text and functionResponse parts are supported"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
