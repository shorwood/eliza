POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 2048,
  "thinking": {"type": "adaptive", "display": "omitted"},
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.content" count == 1
jsonpath "$.content[0].type" == "text"
jsonpath "$.usage.output_tokens" == 8
