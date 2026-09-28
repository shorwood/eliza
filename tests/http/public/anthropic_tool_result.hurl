POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-doctor",
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
jsonpath "$.content[0].text" == "TOOL CALL COMPLETE"
