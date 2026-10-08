POST {{base_url}}/gemini/v1beta/models/fnv-embed:batchEmbedContents
Content-Type: application/json
{"requests": []}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::empty_batch"
jsonpath "$.error.status" == "INVALID_ARGUMENT"
