POST {{base_url}}/ollama/api/embed
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"embed-one","input":"Hello world","dimensions":8}
HTTP 200
[Asserts]
jsonpath "$.model" == "embed-one"
jsonpath "$.embeddings[0][1]" == -0.48507127
