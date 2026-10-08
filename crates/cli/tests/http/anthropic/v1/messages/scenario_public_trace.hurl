POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 2048,
  "thinking": {"type": "enabled", "budget_tokens": 1024},
  "output_config": {"effort": "high"},
  "messages": [{"role": "user", "content": "I am sad"}]
}
HTTP 200
[Asserts]
jsonpath "$.content" count == 2
jsonpath "$.content[0].type" == "thinking"
jsonpath "$.content[0].thinking" == "NORMALIZED INPUT: YOU ARE SAD\nRANKED KEYWORDS: I AM\nSELECTED RULE: I/1/0\nRESPONSE SOURCE: KEYWORD"
jsonpath "$.content[0].signature" == "hJ1c6IwuPTM="
jsonpath "$.content[1].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.usage.output_tokens" == 23
