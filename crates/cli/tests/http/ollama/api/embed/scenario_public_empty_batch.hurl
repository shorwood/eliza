POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "fnv-embed", "input": {"text": "Hello"}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::unsupported_embedding_input"
jsonpath "$.error" == "embedding input must be a string or an array of strings"
