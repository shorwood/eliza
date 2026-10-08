POST {{base_url}}/openai/v1/chat/completions
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "messages": [{"role": "user", "content": [
    {"type": "text", "text": "I am sad"},
    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg==", "detail": "auto"}}
  ]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "[RETRO "
body contains "FOREGROUND]"
body contains "data: [DONE]"
