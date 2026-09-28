POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "messages": [{"role": "user", "content": "I am sad"}],
  "future_field": true
}
HTTP 200
[Asserts]
jsonpath "$.type" == "message"
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.stop_reason" == "end_turn"
