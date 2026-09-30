GET {{base_url}}/ollama/api/tags
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$" isObject
jsonpath "$.models" count == 2
jsonpath "$.models[0].name" == "eliza-1966"
jsonpath "$.models[0].model" == "eliza-1966"
jsonpath "$.models[0].details.family" == "eliza"

POST {{base_url}}/ollama/api/embed
Authorization: Bearer {{token}}
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 4}
HTTP 200
[Asserts]
jsonpath "$.embeddings[0]" count == 4
