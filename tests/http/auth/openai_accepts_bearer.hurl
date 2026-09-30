GET {{base_url}}/openai/v1/models
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 2
jsonpath "$.object" == "list"
jsonpath "$.data" count == 3
jsonpath "$.data[0].id" == "eliza-1966"
jsonpath "$.data[0].object" == "model"

POST {{base_url}}/openai/v1/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 4}
HTTP 200
[Asserts]
jsonpath "$.data[0].embedding" count == 4

POST {{base_url}}/gemini/v1beta/openai/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 4}
HTTP 200
[Asserts]
jsonpath "$.data[0].embedding" count == 4
