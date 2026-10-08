POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 0}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::invalid_dimensions"
jsonpath "$.error" == "embedding dimensions must be between 1 and 1024, got 0"
