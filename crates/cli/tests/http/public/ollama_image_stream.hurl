POST {{base_url}}/ollama/api/chat
Content-Type: application/json
{
  "model": "eliza-1966",
  "messages": [{"role": "user", "content": "I am sad", "images": ["iVBORw0KGgoAAAANSUhEUgAAAAIAAAABAQAAAADcWUInAAAACklEQVQI12NwAAAAQgBBg7nsrQAAAABJRU5ErkJggg=="]}]
}
HTTP 200
[Asserts]
header "Content-Type" startsWith "application/x-ndjson"
body contains "[RETRO "
body contains "FOREGROUND]"
body contains "\"done\":true"
