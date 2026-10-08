POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": [], "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::empty_batch"
jsonpath "$.error.param" == "input"
