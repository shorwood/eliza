POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"inlineData": {"mimeType": "image/png", "data": "AA=="}}]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_text_only"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
