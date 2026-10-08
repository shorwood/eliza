POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
x-goog-api-key: {{token}}
Content-Type: application/json
{
  "model": "models/fnv-embed",
  "content": {"parts": [{"text": "Hello"}]},
  "outputDimensionality": 4
}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 4
