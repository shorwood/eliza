POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 0}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::invalid_dimensions"
jsonpath "$.error.param" == "dimensions"
