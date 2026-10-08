POST {{base_url}}/ollama/api/embed
Authorization: Bearer {{token}}
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 4}
HTTP 200
[Asserts]
jsonpath "$.embeddings[0]" count == 4
