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
