POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/eliza-1966",
  "content": {"parts": [{"text": "Hello"}]}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::embedding_model_required"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
