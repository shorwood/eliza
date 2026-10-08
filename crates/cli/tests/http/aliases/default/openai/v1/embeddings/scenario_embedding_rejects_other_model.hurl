POST {{base_url}}/openai/v1/embeddings
Authorization: Bearer {{token}}
Content-Type: application/json
{"model":"fnv-embed","input":"Hello world"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error.message" contains "embed-one"
