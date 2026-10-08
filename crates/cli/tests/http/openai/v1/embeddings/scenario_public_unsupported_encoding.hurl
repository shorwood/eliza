POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "encoding_format": "base64"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_embedding_encoding"
jsonpath "$.error.param" == "encoding_format"
