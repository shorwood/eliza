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
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 8
jsonpath "$.id" startsWith "msg_"
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.model" == "eliza-doctor"
jsonpath "$.content" count == 1
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" == "I AM SORRY TO HEAR YOU ARE SAD"
jsonpath "$.stop_reason" == "end_turn"
jsonpath "$.stop_sequence" == null
jsonpath "$.usage.input_tokens" == 3
jsonpath "$.usage.output_tokens" == 8
