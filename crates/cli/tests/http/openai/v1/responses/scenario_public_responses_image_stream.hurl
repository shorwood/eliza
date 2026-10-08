POST {{base_url}}/openai/v1/responses
Content-Type: application/json
{
  "model": "eliza-1966",
  "stream": true,
  "input": [{"type": "message", "role": "user", "content": [
    {"type": "input_text", "text": "I am sad"},
    {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}
  ]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "[RETRO "
body contains "FOREGROUND]"
body contains "\"type\":\"response.completed\""
