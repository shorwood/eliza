GET {{base_url}}/gemini/v1beta/models
x-goog-api-key: {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 3
jsonpath "$.models[0].name" == "models/eliza-1966"
jsonpath "$.models[0].version" == "1966-doctor"
jsonpath "$.models[0].supportedGenerationMethods" count == 2

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
