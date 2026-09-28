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
header "Content-Type" startsWith "text/event-stream"
body contains "\"object\":\"chat.completion.chunk\""
body contains "\"model\":\"eliza-doctor\""
body contains "\"tool_calls\""
body contains "\"name\":\"echo\""
body contains "\"arguments\":\"{\\\"value\\\":\\\"hello\\\"}\""
body contains "\"finish_reason\":\"tool_calls\""
body contains "\"prompt_tokens\""
body contains "data: [DONE]"
