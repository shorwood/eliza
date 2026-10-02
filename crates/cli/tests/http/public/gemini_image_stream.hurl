POST {{base_url}}/gemini/v1beta/models/eliza-1966:streamGenerateContent?alt=sse
Content-Type: application/json
{"contents": [{"role": "user", "parts": [
  {"text": "I am sad"},
  {"inlineData": {"mimeType": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="}}
]}]}
HTTP 200
[Asserts]
header "Content-Type" startsWith "text/event-stream"
body contains "[RETRO "
body contains "FOREGROUND]"
body contains "\"finishReason\":\"STOP\""
