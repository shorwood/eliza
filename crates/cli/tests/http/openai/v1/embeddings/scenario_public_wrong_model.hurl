POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error.param" == "model"
