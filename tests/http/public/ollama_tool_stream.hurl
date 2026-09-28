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
body contains "\"tool_calls\""
body contains "\"name\":\"echo\""
body contains "\"done\":true"
