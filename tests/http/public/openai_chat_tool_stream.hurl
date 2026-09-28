POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "stream": true,
  "stream_options": {"include_usage": true},
  "tools": [{
    "type": "function",
    "function": {"name": "echo", "parameters": {"type": "object"}}
  }],
  "messages": [{"role": "user", "content": "@tool echo {\"value\":\"hello\"}"}]
}
HTTP 200
[Asserts]
body contains "tool_calls"
body contains "prompt_tokens"
body contains "data: [DONE]"
