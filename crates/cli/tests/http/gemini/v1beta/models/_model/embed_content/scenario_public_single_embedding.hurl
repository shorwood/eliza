POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello world"}]},
  "taskType": "SEMANTIC_SIMILARITY",
  "title": "ignored",
  "outputDimensionality": 8
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[0]" == 0.0
jsonpath "$.embedding.values[1]" == -0.48507127
jsonpath "$.embedding.values[3]" == 0.24253564
jsonpath "$.embedding.values[7]" == 0.7276069
jsonpath "$.usageMetadata.promptTokenCount" == 2
