POST {{base_url}}/gemini/v1beta/models/eliza-1966:generateContent
Content-Type: application/json
{
  "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
  "generationConfig": {
    "responseFormat": {"text": {"mimeType": "TEXT_PLAIN"}},
    "responseMimeType": "application/json"
  }
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::conflicting_response_formats"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
