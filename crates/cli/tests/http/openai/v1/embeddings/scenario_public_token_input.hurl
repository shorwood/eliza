POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": [1, 2, 3]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::embedding_token_input_unsupported"
jsonpath "$.error.param" == "input"
