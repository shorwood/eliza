POST {{base_url}}/anthropic/v1/messages
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "stream": true,
  "tools": [{"name": "echo", "input_schema": {"type": "object"}}],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
body contains "input_json_delta"
body contains "tool_use"
