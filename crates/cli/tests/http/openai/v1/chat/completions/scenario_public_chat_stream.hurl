POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "\"object\":\"chat.completion.chunk\""
body contains "\"model\":\"eliza-1966\""
body contains "\"role\":\"assistant\""
body contains "\"finish_reason\":\"stop\""
body contains "data: [DONE]"
