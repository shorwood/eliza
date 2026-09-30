POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{
  "model": "fnv-embed",
  "input": ["Hello world", "Second text"],
  "dimensions": 8
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.model" == "fnv-embed"
jsonpath "$.embeddings" count == 2
jsonpath "$.embeddings[0]" count == 8
jsonpath "$.embeddings[0][0]" == 0.0
jsonpath "$.embeddings[0][1]" == -0.48507127
jsonpath "$.embeddings[0][3]" == 0.24253564
jsonpath "$.embeddings[0][7]" == 0.7276069
jsonpath "$.embeddings[1]" count == 8
jsonpath "$.total_duration" == 0
jsonpath "$.load_duration" == 0
jsonpath "$.prompt_eval_count" == 4

POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "fnv-embed", "input": {"text": "Hello"}}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::ollama::unsupported_embedding_input"
jsonpath "$.error" == "embedding input must be a string or an array of strings"

POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "fnv-embed", "input": "Hello", "dimensions": 0}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::invalid_dimensions"
jsonpath "$.error" == "embedding dimensions must be between 1 and 1024, got 0"

POST {{base_url}}/ollama/api/embed
Content-Type: application/json
{"model": "eliza-1966", "input": "Hello", "dimensions": 8}
HTTP 400
[Asserts]
header "x-eliza-error-code" == "eliza::embedding::model_required"
jsonpath "$.error" == "embeddings require model `fnv-embed`"
