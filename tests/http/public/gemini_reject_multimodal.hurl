POST {{base_url}}/gemini/v1beta/models/eliza-doctor:generateContent
Content-Type: application/json
{
  "contents": [{
    "role": "user",
    "parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]
  }]
}
HTTP 400
[Asserts]
jsonpath "$.error.status" == "INVALID_ARGUMENT"
