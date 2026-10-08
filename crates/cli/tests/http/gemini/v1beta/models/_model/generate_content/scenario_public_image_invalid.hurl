POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{"contents": [{"role": "user", "parts": [{"inlineData": {"mimeType": "image/gif", "data": "R0lGODlh"}}]}]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::image::unsupported_media_type"
jsonpath "$.error.code" == 400
jsonpath "$.error.status" == "INVALID_ARGUMENT"
