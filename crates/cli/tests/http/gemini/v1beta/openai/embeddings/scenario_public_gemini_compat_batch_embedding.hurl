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
