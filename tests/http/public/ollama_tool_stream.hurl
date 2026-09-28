POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-doctor",
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "\"model\":\"eliza-doctor\""
body contains "\"created_at\":\"1966-01-01T00:00:00Z\""
body contains "\"role\":\"assistant\""
body contains "\"tool_calls\""
body contains "\"type\":\"function\""
body contains "\"name\":\"echo\""
body contains "\"arguments\":{\"value\":\"hello\"}"
body contains "\"done\":false"
body contains "\"done\":true"
body contains "\"done_reason\":\"stop\""
