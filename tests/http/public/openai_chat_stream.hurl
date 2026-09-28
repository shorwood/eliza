POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-doctor",
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "chat.completion.chunk"
body contains "data: [DONE]"
