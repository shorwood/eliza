# Legacy chat renaming may overlap a dedicated model's default name.
GET {{base_url}}/openai/v1/models
HTTP 200
[Asserts]
jsonpath "$.data" count == 3
jsonpath "$.data[0].id" == "fnv-embed"

GET {{base_url}}/gemini/v1beta/models
HTTP 200
[Asserts]
jsonpath "$.models" count == 3
jsonpath "$.models[0].name" == "models/fnv-embed"
jsonpath "$.models[0].displayName" == "ELIZA DOCTOR"

GET {{base_url}}/ollama/api/tags
HTTP 200
[Asserts]
jsonpath "$.models" count == 1
jsonpath "$.models[0].details.family" == "eliza"

POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{"model":"fnv-embed","messages":[{"role":"user","content":"I need help"}]}
HTTP 200
[Asserts]
jsonpath "$.model" == "fnv-embed"
jsonpath "$.choices[0].message.content" != ""

POST {{base_url}}/gemini/v1beta/models/fnv-embed:embedContent
Content-Type: application/json
{"model":"models/fnv-embed","content":{"parts":[{"text":"Hello world"}]},"outputDimensionality":8}
HTTP 200
[Asserts]
jsonpath "$.embedding.values" count == 8
jsonpath "$.embedding.values[1]" == -0.48507127
