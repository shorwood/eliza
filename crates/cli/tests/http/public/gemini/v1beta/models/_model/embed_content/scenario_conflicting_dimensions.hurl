POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello"}]},
  "outputDimensionality": 8,
  "embedContentConfig": {"outputDimensionality": 4}
}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::gemini::conflicting_embedding_dimensions"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
