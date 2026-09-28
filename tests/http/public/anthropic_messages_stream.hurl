POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-doctor",
  "max_tokens": 128,
  "stream": true,
  "messages": [{"role": "user", "content": "Hello"}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "event: message_start"
body contains "event: content_block_delta"
body contains "event: message_stop"
