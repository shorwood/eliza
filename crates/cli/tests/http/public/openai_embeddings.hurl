POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{
  "model": "fnv-embed",
  "input": "Hello world",
  "dimensions": 8,
  "encoding_format": "float",
  "user": "fixture-user"
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.object" == "list"
jsonpath "$.data" count == 1
jsonpath "$.data[0].object" == "embedding"
jsonpath "$.data[0].index" == 0
jsonpath "$.data[0].embedding" count == 8
jsonpath "$.data[0].embedding[0]" == 0.0
jsonpath "$.data[0].embedding[1]" == -0.48507127
jsonpath "$.data[0].embedding[3]" == 0.24253564
jsonpath "$.data[0].embedding[7]" == 0.7276069
jsonpath "$.model" == "fnv-embed"
jsonpath "$.usage.prompt_tokens" == 2
jsonpath "$.usage.total_tokens" == 2

POST {{base_url}}/gemini/v1beta/openai/embeddings
Content-Type: application/json
{
  "model": "fnv-embed",
  "input": ["Hello world", "Second text"],
  "dimensions": 8
}
HTTP 200
[Asserts]
jsonpath "$.data" count == 2
jsonpath "$.data[0].index" == 0
jsonpath "$.data[0].embedding[1]" == -0.48507127
jsonpath "$.data[1].index" == 1
jsonpath "$.data[1].embedding" count == 8
jsonpath "$.usage.prompt_tokens" == 4
jsonpath "$.usage.total_tokens" == 4

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "encoding_format": "base64"}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::unsupported_embedding_encoding"
jsonpath "$.error.param" == "encoding_format"

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": [1, 2, 3]}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::openai::embedding_token_input_unsupported"
jsonpath "$.error.param" == "input"

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": [], "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::empty_batch"
jsonpath "$.error.param" == "input"

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": "...", "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::blank_input"
jsonpath "$.error.param" == "input"

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 1025}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::invalid_dimensions"
jsonpath "$.error.param" == "dimensions"

POST {{base_url}}/openai/v1/embeddings
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error.param" == "model"
