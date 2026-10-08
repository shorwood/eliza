POST {{base_url}}/anthropic/v1/messages
anthropic-version: 2023-06-01
Content-Type: application/json
{
  "model": "eliza-1966",
  "max_tokens": 128,
  "stream": true,
  "messages": [{"role": "user", "content": [
    {"type": "text", "text": "I am sad"},
    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}
  ]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "[RETRO "
body contains "FOREGROUND]"
body contains "event: message_stop"
