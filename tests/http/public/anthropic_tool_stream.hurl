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
header "Content-Type" startsWith "text/event-stream"
body contains "event: message_start"
body contains "\"model\":\"eliza-doctor\""
body contains "event: content_block_start"
body contains "\"type\":\"tool_use\""
body contains "\"name\":\"echo\""
body contains "event: content_block_delta"
body contains "\"type\":\"input_json_delta\""
body contains "\"partial_json\":\"{\\\"value\\\":\\\"hello\\\"}\""
body contains "event: content_block_stop"
body contains "\"stop_reason\":\"tool_use\""
body contains "event: message_stop"
