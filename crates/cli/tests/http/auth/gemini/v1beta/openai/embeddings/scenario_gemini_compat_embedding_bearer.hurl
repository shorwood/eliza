POST {{base_url}}/gemini/v1beta/openai/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 4}
HTTP 200
[Asserts]
jsonpath "$.data[0].embedding" count == 4
