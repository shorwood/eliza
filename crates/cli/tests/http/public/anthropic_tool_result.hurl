POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "tools": [{"name": "echo", "input_schema": {"type": "object"}}],
  "messages": [
    {"role": "user", "content": "@tool echo {\"value\":\"hello\"}"},
    {"role": "assistant", "content": [{
      "type": "tool_use",
      "id": "toolu_fixture",
      "name": "echo",
      "input": {"value": "hello"}
    }]},
    {"role": "user", "content": [{
      "type": "tool_result",
      "tool_use_id": "toolu_fixture",
      "content": "hello"
    }]}
  ]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/json"
jsonpath "$.*" count == 8
jsonpath "$.id" startsWith "msg_"
jsonpath "$.type" == "message"
jsonpath "$.role" == "assistant"
jsonpath "$.model" == "eliza-1966"
jsonpath "$.content" count == 1
jsonpath "$.content[0].type" == "text"
jsonpath "$.content[0].text" == "TOOL CALL COMPLETE"
jsonpath "$.stop_reason" == "end_turn"
jsonpath "$.stop_sequence" == null
jsonpath "$.usage.input_tokens" isInteger
jsonpath "$.usage.output_tokens" isInteger
