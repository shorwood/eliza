# Embedding aliases still produce the deterministic vectors.
POST {{base_url}}/openai/v1/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"embed-two","input":"Hello world","dimensions":8}
HTTP 200
[Asserts]
jsonpath "$.model" == "embed-two"
jsonpath "$.data[0].embedding" count == 8
jsonpath "$.data[0].embedding[1]" == -0.48507127
