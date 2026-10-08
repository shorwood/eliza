POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error" == "embeddings require model `fnv-embed`"
