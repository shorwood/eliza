POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "tools": [{
    "name": "echo",
    "description": "Echo a value",
    "input_schema": {"type": "object"}
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
jsonpath "$.content[0].type" == "tool_use"
jsonpath "$.content[0].input.value" == "hello"
jsonpath "$.stop_reason" == "tool_use"
